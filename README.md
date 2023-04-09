# Pattern matching library
Allows you to search for a pattern within data via an iterator interface.

This library uses the core::simd abstraction and does not allocate.

## Usage
```rs
use patterns::Pattern;

let data = [0_u8; 1_000_00];
// Allows . and ? as wildcard.
// Any number of wildcard characters between spaces is considered a wildcard byte.
let pattern: Pattern = "01 02 00 ? 59 ff".parse().unwrap();
let mut iterator = pattern.matches(&data);

for _found in iterator {
    // use _found
}
```
