// Copyright © 2022 Intel Corporation
//
// SPDX-License-Identifier: Apache-2.0
//

#[macro_export]
macro_rules! trace_relative_scoped {
    ($event:expr, $size:expr) => {};
}

#[macro_export]
macro_rules! trace_relative_point {
    ($event:expr) => {};
}

pub fn end_relative() {}
pub fn start_relative() {}