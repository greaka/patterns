#![no_main]

mod common;

use common::{run_test, FuzzData};
use libfuzzer_sys::{fuzz_target, Corpus};

fuzz_target!(|input: FuzzData| -> Corpus {
    run_test(input);

    Corpus::Keep
});
