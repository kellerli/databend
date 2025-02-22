// Copyright 2020-2022 Jorge C. Leitão
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

use common_arrow::arrow::array::growable::Growable;
use common_arrow::arrow::array::growable::GrowableBinary;
use common_arrow::arrow::array::BinaryArray;

#[test]
fn no_offsets() {
    let array = BinaryArray::<i32>::from([Some("a"), Some("bc"), None, Some("defh")]);

    let mut a = GrowableBinary::new(vec![&array], false, 0);

    a.extend(0, 1, 2);
    assert_eq!(a.len(), 2);

    let result: BinaryArray<i32> = a.into();

    let expected = BinaryArray::<i32>::from([Some("bc"), None]);
    assert_eq!(result, expected);
}

/// tests extending from a variable-sized (strings and binary) array
/// with an offset and nulls
#[test]
fn with_offsets() {
    let array = BinaryArray::<i32>::from([Some("a"), Some("bc"), None, Some("defh")]);
    let array = array.sliced(1, 3);

    let mut a = GrowableBinary::new(vec![&array], false, 0);

    a.extend(0, 0, 3);
    assert_eq!(a.len(), 3);

    let result: BinaryArray<i32> = a.into();

    let expected = BinaryArray::<i32>::from([Some("bc"), None, Some("defh")]);
    assert_eq!(result, expected);
}

#[test]
fn test_string_offsets() {
    let array = BinaryArray::<i32>::from([Some("a"), Some("bc"), None, Some("defh")]);
    let array = array.sliced(1, 3);

    let mut a = GrowableBinary::new(vec![&array], false, 0);

    a.extend(0, 0, 3);
    assert_eq!(a.len(), 3);

    let result: BinaryArray<i32> = a.into();

    let expected = BinaryArray::<i32>::from([Some("bc"), None, Some("defh")]);
    assert_eq!(result, expected);
}

#[test]
fn test_multiple_with_validity() {
    let array1 = BinaryArray::<i32>::from_slice([b"hello", b"world"]);
    let array2 = BinaryArray::<i32>::from([Some("1"), None]);

    let mut a = GrowableBinary::new(vec![&array1, &array2], false, 5);

    a.extend(0, 0, 2);
    a.extend(1, 0, 2);
    assert_eq!(a.len(), 4);

    let result: BinaryArray<i32> = a.into();

    let expected = BinaryArray::<i32>::from([Some("hello"), Some("world"), Some("1"), None]);
    assert_eq!(result, expected);
}

#[test]
fn test_string_null_offset_validity() {
    let array = BinaryArray::<i32>::from([Some("a"), Some("bc"), None, Some("defh")]);
    let array = array.sliced(1, 3);

    let mut a = GrowableBinary::new(vec![&array], true, 0);

    a.extend(0, 1, 2);
    a.extend_validity(1);
    assert_eq!(a.len(), 3);

    let result: BinaryArray<i32> = a.into();

    let expected = BinaryArray::<i32>::from([None, Some("defh"), None]);
    assert_eq!(result, expected);
}
