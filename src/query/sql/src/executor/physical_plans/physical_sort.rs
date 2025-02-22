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

use common_exception::Result;
use common_expression::types::DataType;
use common_expression::DataField;
use common_expression::DataSchema;
use common_expression::DataSchemaRef;
use common_expression::DataSchemaRefExt;
use itertools::Itertools;

use crate::executor::explain::PlanStatsInfo;
use crate::executor::physical_plans::common::SortDesc;
use crate::executor::PhysicalPlan;
use crate::executor::PhysicalPlanBuilder;
use crate::optimizer::SExpr;
use crate::ColumnSet;
use crate::IndexType;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Sort {
    /// A unique id of operator in a `PhysicalPlan` tree, only used for display.
    pub plan_id: u32,
    pub input: Box<PhysicalPlan>,
    pub order_by: Vec<SortDesc>,
    /// limit = Limit.limit + Limit.offset
    pub limit: Option<usize>,
    /// If the sort plan is after the exchange plan.
    /// It's [None] if the sorting plan is in single node mode.
    pub after_exchange: Option<bool>,
    pub pre_projection: Option<Vec<IndexType>>,

    // Only used for explain
    pub stat_info: Option<PlanStatsInfo>,
}

impl Sort {
    fn order_col_type(&self, schema: &DataSchema) -> Result<DataType> {
        if self.order_by.len() == 1 {
            let order_by_field = schema.field_with_name(&self.order_by[0].order_by.to_string())?;
            if matches!(
                order_by_field.data_type(),
                DataType::Number(_) | DataType::Date | DataType::Timestamp | DataType::String
            ) {
                return Ok(order_by_field.data_type().clone());
            }
        }
        Ok(DataType::String)
    }

    pub fn output_schema(&self) -> Result<DataSchemaRef> {
        let input_schema = self.input.output_schema()?;
        let mut fields = input_schema.fields().clone();
        if matches!(self.after_exchange, Some(true)) {
            // If the plan is after exchange plan in cluster mode,
            // the order column is at the last of the input schema.
            debug_assert_eq!(fields.last().unwrap().name(), "_order_col");
            debug_assert_eq!(
                fields.last().unwrap().data_type(),
                &self.order_col_type(&input_schema)?
            );
            fields.pop();
        } else {
            if let Some(proj) = &self.pre_projection {
                let fileted_fields = proj
                    .iter()
                    .filter_map(|index| input_schema.field_with_name(&index.to_string()).ok())
                    .cloned()
                    .collect::<Vec<_>>();
                if fileted_fields.len() < fields.len() {
                    // Only if the projection is not a full projection, we need to add a projection transform.
                    fields = fileted_fields
                }
            }

            if matches!(self.after_exchange, Some(false)) {
                // If the plan is before exchange plan in cluster mode,
                // the order column should be added to the output schema.
                fields.push(DataField::new(
                    "_order_col",
                    self.order_col_type(&input_schema)?,
                ));
            }
        }

        Ok(DataSchemaRefExt::create(fields))
    }
}

impl PhysicalPlanBuilder {
    pub(crate) async fn build_sort(
        &mut self,
        s_expr: &SExpr,
        sort: &crate::plans::Sort,
        mut required: ColumnSet,
        stat_info: PlanStatsInfo,
    ) -> Result<PhysicalPlan> {
        // 1. Prune unused Columns.
        sort.items.iter().for_each(|s| {
            required.insert(s.index);
        });

        // If the query will be optimized by lazy reading, we don't need to do pre-projection.
        let pre_projection = if self.metadata.read().lazy_columns().is_empty() {
            Some(required.iter().sorted().copied().collect())
        } else {
            None
        };

        // 2. Build physical plan.
        Ok(PhysicalPlan::Sort(Sort {
            plan_id: self.next_plan_id(),
            input: Box::new(self.build(s_expr.child(0)?, required).await?),
            order_by: sort
                .items
                .iter()
                .map(|v| SortDesc {
                    asc: v.asc,
                    nulls_first: v.nulls_first,
                    order_by: v.index,
                })
                .collect(),
            limit: sort.limit,
            after_exchange: sort.after_exchange,
            pre_projection,
            stat_info: Some(stat_info),
        }))
    }
}
