# vv

`Vv<T>` is a lot like a `Vec<Vec<T>>` except contiguous in memory. It is unlike other
contiguous "jagged array" implementations in that it allows opportunistic growing and
shrinking of inner vecs, sometimes requirement moving an entry to achieve the change.

Please consult [**the documentation**](https://docs.rs/vv) for more information.

Add it to your Cargo.toml:

```toml
[dependencies]
vv = "0.1"
```

# Example

```rs
use vv::Vv;

let mut vv = Vv::<i32>::new();
let first = vv.push([1, 2, 3]);
let second = vv.push([7, 8, 9]);
vv.get_mut(first).rotate_right();
let first = vv.insert(first, 0, 1);

assert_eq!(vv.get(first), [1, 3, 1, 2]);
```

## License

`vv` is dual-licensed under either:

* MIT License ([LICENSE-MIT](LICENSE-MIT) or [http://opensource.org/licenses/MIT](http://opensource.org/licenses/MIT))
* Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or [http://www.apache.org/licenses/LICENSE-2.0](http://www.apache.org/licenses/LICENSE-2.0))

at your option.

### Your contributions

Unless you explicitly state otherwise,
any contribution intentionally submitted for inclusion in the work by you,
as defined in the Apache-2.0 license,
shall be dual licensed as above,
without any additional terms or conditions.