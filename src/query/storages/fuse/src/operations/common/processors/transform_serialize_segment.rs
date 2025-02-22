// Copyright 2021 Datafuse Labs
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::any::Any;
use std::sync::Arc;

use async_trait::async_trait;
use common_catalog::table_context::TableContext;
use common_exception::ErrorCode;
use common_exception::Result;
use common_expression::BlockMetaInfoDowncast;
use common_expression::BlockThresholds;
use common_expression::DataBlock;
use common_pipeline_core::processors::Event;
use common_pipeline_core::processors::InputPort;
use common_pipeline_core::processors::OutputPort;
use common_pipeline_core::processors::Processor;
use common_pipeline_core::processors::ProcessorPtr;
use common_pipeline_core::PipeItem;
use log::info;
use opendal::Operator;
use storages_common_cache::CacheAccessor;
use storages_common_cache_manager::CachedObject;
use storages_common_table_meta::meta::BlockMeta;
use storages_common_table_meta::meta::SegmentInfo;
use storages_common_table_meta::meta::Versioned;

use crate::io::TableMetaLocationGenerator;
use crate::operations::common::AbortOperation;
use crate::operations::common::MutationLogEntry;
use crate::operations::common::MutationLogs;
use crate::statistics::StatisticsAccumulator;
use crate::FuseTable;
use crate::DEFAULT_BLOCK_PER_SEGMENT;
use crate::FUSE_OPT_KEY_BLOCK_PER_SEGMENT;

enum State {
    None,
    GenerateSegment,
    SerializedSegment {
        data: Vec<u8>,
        location: String,
        segment: Arc<SegmentInfo>,
    },
    PreCommitSegment {
        location: String,
        segment: Arc<SegmentInfo>,
    },
    Finished,
}

pub struct TransformSerializeSegment {
    ctx: Arc<dyn TableContext>,
    data_accessor: Operator,
    meta_locations: TableMetaLocationGenerator,
    accumulator: StatisticsAccumulator,
    state: State,
    input: Arc<InputPort>,
    output: Arc<OutputPort>,
    output_data: Option<DataBlock>,
    block_per_seg: u64,

    thresholds: BlockThresholds,
    default_cluster_key_id: Option<u32>,
}

impl TransformSerializeSegment {
    pub fn new(
        ctx: Arc<dyn TableContext>,
        input: Arc<InputPort>,
        output: Arc<OutputPort>,
        table: &FuseTable,
        thresholds: BlockThresholds,
    ) -> Self {
        let default_cluster_key_id = table.cluster_key_id();
        TransformSerializeSegment {
            ctx,
            input,
            output,
            output_data: None,
            data_accessor: table.get_operator(),
            meta_locations: table.meta_location_generator().clone(),
            state: State::None,
            accumulator: Default::default(),
            block_per_seg: table
                .get_option(FUSE_OPT_KEY_BLOCK_PER_SEGMENT, DEFAULT_BLOCK_PER_SEGMENT)
                as u64,
            thresholds,
            default_cluster_key_id,
        }
    }

    pub fn into_processor(self) -> Result<ProcessorPtr> {
        Ok(ProcessorPtr::create(Box::new(self)))
    }

    pub fn into_pipe_item(self) -> PipeItem {
        let input = self.input.clone();
        let output = self.output.clone();
        let processor_ptr = ProcessorPtr::create(Box::new(self));
        PipeItem::create(processor_ptr, vec![input], vec![output])
    }
}

#[async_trait]
impl Processor for TransformSerializeSegment {
    fn name(&self) -> String {
        "TransformSerializeSegment".to_string()
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    fn event(&mut self) -> Result<Event> {
        if matches!(
            &self.state,
            State::GenerateSegment | State::PreCommitSegment { .. }
        ) {
            return Ok(Event::Sync);
        }

        if matches!(&self.state, State::SerializedSegment { .. }) {
            return Ok(Event::Async);
        }

        if self.output.is_finished() {
            return Ok(Event::Finished);
        }

        if !self.output.can_push() {
            return Ok(Event::NeedConsume);
        }

        if let Some(data_block) = self.output_data.take() {
            self.output.push_data(Ok(data_block));
            return Ok(Event::NeedConsume);
        }

        if self.input.is_finished() {
            if self.accumulator.summary_row_count != 0 {
                self.state = State::GenerateSegment;
                return Ok(Event::Sync);
            }
            self.output.finish();
            self.state = State::Finished;
            return Ok(Event::Finished);
        }

        if self.input.has_data() {
            let input_meta = self
                .input
                .pull_data()
                .unwrap()?
                .get_meta()
                .cloned()
                .ok_or_else(|| ErrorCode::Internal("No block meta. It's a bug"))?;
            let block_meta = BlockMeta::downcast_ref_from(&input_meta)
                .ok_or_else(|| ErrorCode::Internal("No commit meta. It's a bug"))?
                .clone();

            self.accumulator.add_with_block_meta(block_meta);
            if self.accumulator.summary_block_count >= self.block_per_seg {
                self.state = State::GenerateSegment;
                return Ok(Event::Sync);
            }
        }

        self.input.set_need_data();
        Ok(Event::NeedData)
    }

    fn process(&mut self) -> Result<()> {
        match std::mem::replace(&mut self.state, State::None) {
            State::GenerateSegment => {
                let acc = std::mem::take(&mut self.accumulator);
                let summary = acc.summary(self.thresholds, self.default_cluster_key_id);

                let segment_info = SegmentInfo::new(acc.blocks_metas, summary);

                self.state = State::SerializedSegment {
                    data: segment_info.to_bytes()?,
                    location: self.meta_locations.gen_segment_info_location(),
                    segment: Arc::new(segment_info),
                }
            }
            State::PreCommitSegment { location, segment } => {
                if let Some(segment_cache) = SegmentInfo::cache() {
                    segment_cache.put(location.clone(), Arc::new(segment.as_ref().try_into()?));
                }

                let mut abort_operation = AbortOperation::default();
                for block_meta in &segment.blocks {
                    abort_operation.add_block(block_meta);
                }
                abort_operation.add_segment(location.clone());

                let format_version = SegmentInfo::VERSION;

                // emit log entry.
                // for newly created segment, always use the latest version
                let meta = MutationLogs {
                    entries: vec![MutationLogEntry::AppendSegment {
                        segment_location: location.clone(),
                        format_version,
                        abort_operation,
                        summary: segment.summary.clone(),
                    }],
                };

                self.ctx.add_segment_location((location, format_version))?;

                self.output_data = Some(DataBlock::empty_with_meta(Box::new(meta)));
            }
            _state => {
                return Err(ErrorCode::Internal("Unknown state for fuse table sink"));
            }
        }

        Ok(())
    }

    #[async_backtrace::framed]
    async fn async_process(&mut self) -> Result<()> {
        match std::mem::replace(&mut self.state, State::None) {
            State::SerializedSegment {
                data,
                location,
                segment,
            } => {
                self.data_accessor.write(&location, data).await?;
                info!("fuse append wrote down segment {} ", location);

                self.state = State::PreCommitSegment { location, segment };
            }
            _state => {
                return Err(ErrorCode::Internal("Unknown state for fuse table sink."));
            }
        }

        Ok(())
    }
}
