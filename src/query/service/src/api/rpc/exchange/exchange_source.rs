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

use std::sync::Arc;

use common_catalog::table_context::TableContext;
use common_exception::ErrorCode;
use common_exception::Result;
use common_pipeline_core::processors::InputPort;
use common_pipeline_core::processors::OutputPort;
use common_pipeline_core::Pipe;
use common_pipeline_core::PipeItem;
use common_pipeline_core::Pipeline;
use common_pipeline_transforms::processors::TransformDummy;

use crate::api::rpc::exchange::exchange_params::ExchangeParams;
use crate::api::rpc::exchange::exchange_params::MergeExchangeParams;
use crate::api::rpc::exchange::exchange_source_reader::ExchangeSourceReader;
use crate::api::ExchangeInjector;
use crate::clusters::ClusterHelper;
use crate::sessions::QueryContext;

/// Add Exchange Source to the pipeline.
pub fn via_exchange_source(
    ctx: Arc<QueryContext>,
    params: &MergeExchangeParams,
    injector: Arc<dyn ExchangeInjector>,
    pipeline: &mut Pipeline,
) -> Result<()> {
    // UpstreamTransform --->  DummyTransform   --->    DummyTransform      --->  DownstreamTransform
    //      ...          --->      ...          --->        ...             --->        ...
    // UpstreamTransform --->  DummyTransform   --->    DummyTransform      --->        ...
    //                         ExchangeSource   --->  DeserializeTransform  --->        ...
    //                             ...          --->        ...             --->        ...
    //                         ExchangeSource   --->  DeserializeTransform  --->  DownstreamTransform

    if params.destination_id != ctx.get_cluster().local_id() {
        return Err(ErrorCode::Internal(format!(
            "Locally depends on merge exchange, but the localhost is not a coordination node. executor: {}, destination_id: {}, fragment id: {}",
            ctx.get_cluster().local_id(),
            params.destination_id,
            params.fragment_id
        )));
    }

    let exchange_params = ExchangeParams::MergeExchange(params.clone());
    let exchange_manager = ctx.get_exchange_manager();
    let flight_receivers = exchange_manager.get_flight_receiver(&exchange_params)?;

    let last_output_len = pipeline.output_len();
    let mut items = Vec::with_capacity(last_output_len + flight_receivers.len());

    for _index in 0..last_output_len {
        let input = InputPort::create();
        let output = OutputPort::create();

        items.push(PipeItem::create(
            TransformDummy::create(input.clone(), output.clone()),
            vec![input],
            vec![output],
        ));
    }

    for (destination_id, flight_exchange) in flight_receivers {
        let output = OutputPort::create();
        items.push(PipeItem::create(
            ExchangeSourceReader::create(
                output.clone(),
                flight_exchange,
                &destination_id,
                &ctx.get_cluster().local_id(),
                params.fragment_id,
            ),
            vec![],
            vec![output],
        ));
    }

    pipeline.add_pipe(Pipe::create(last_output_len, items.len(), items));

    if params.allow_adjust_parallelism {
        pipeline.try_resize(last_output_len)?;
    }

    injector.apply_merge_deserializer(params, pipeline)
}
