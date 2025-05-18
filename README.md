# Pattern matching library
Allows you to search for a pattern within data via an iterator interface.
This library uses the core::simd abstraction and is fully no_std, no alloc.

## Usage
```
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
More advanced use cases may also specify a target alignment required to
match, or the LANE size with which to search:

```
use patterns::Pattern;

let _pattern: Pattern<4, 64> = "00 01 02 . ff".parse().unwrap();
```

## Limitations
- The maximum amount of bytes supported inside a pattern are determined by
  the chosen 2nd const parameter (default 64)
- Target alignment of the pattern to search for must be less or equal to
  that 2nd const parameter
- The pointer of data to search through must adhere to these bounds:
  - `data.as_ptr() - 64 > `[`usize::MIN`]
  - `data.as_ptr() + data.len() + 64 < `[`usize::MAX`]

In practice, it's impossible to be outside of these bounds when using an OS.
