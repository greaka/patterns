mod common;
mod data;

use arbitrary::{Arbitrary, Unstructured};
use common::{run_test, FuzzData};

#[test]
fn test() {
    for entry in data::DATA {
        let unstructured = Unstructured::new(entry);
        let input = FuzzData::arbitrary_take_rest(unstructured).unwrap();

        run_test(input);
    }
}
