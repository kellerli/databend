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

use std::collections::BTreeMap;
use std::sync::Arc;

use common_catalog::plan::InternalColumn;
use common_catalog::plan::InternalColumnMeta;
use common_exception::ErrorCode;
use common_exception::Result;
use common_expression::BlockMetaInfoDowncast;
use common_expression::DataBlock;
use common_expression::FieldIndex;
use common_expression::CHANGE_ROW_ID_COLUMN_ID;
use common_pipeline_core::processors::InputPort;
use common_pipeline_core::processors::OutputPort;
use common_pipeline_core::processors::ProcessorPtr;
use common_pipeline_transforms::processors::Transform;
use common_pipeline_transforms::processors::Transformer;

pub struct TransformAddInternalColumns {
    internal_columns: BTreeMap<FieldIndex, InternalColumn>,
}

impl TransformAddInternalColumns
where Self: Transform
{
    pub fn try_create(
        input: Arc<InputPort>,
        output: Arc<OutputPort>,
        internal_columns: BTreeMap<FieldIndex, InternalColumn>,
    ) -> Result<ProcessorPtr> {
        Ok(ProcessorPtr::create(Transformer::create(
            input,
            output,
            Self { internal_columns },
        )))
    }
}

impl Transform for TransformAddInternalColumns {
    const NAME: &'static str = "AddInternalColumnsTransform";

    fn transform(&mut self, mut block: DataBlock) -> Result<DataBlock> {
        if let Some(meta) = block.take_meta() {
            let internal_column_meta =
                InternalColumnMeta::downcast_from(meta).ok_or(ErrorCode::Internal("It's a bug"))?;
            let num_rows = block.num_rows();
            for internal_column in self.internal_columns.values() {
                if internal_column.column_id() == CHANGE_ROW_ID_COLUMN_ID {
                    continue;
                }
                let column =
                    internal_column.generate_column_values(&internal_column_meta, num_rows);
                block.add_column(column);
            }
        }
        Ok(block)
    }
}
