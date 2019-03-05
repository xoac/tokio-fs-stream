# WIP: tokio-fs-stream
Backup sink on disk when it can't be properly flushed. 

## Usage by the example.
Most feature example can be see here `examples/file_unordered.rs`.
Example using unbounded (save to file until os error occure) file: `exmple/async_through_file.rs` 
Example using unbounded dir (save inside dir create new file after inserting some amount of items) `example/async_through_dir.rs`

## TODO for v0.1.0:
* [ ] MR [Sink retry](https://gitlab.com/mexus/futures-retry/merge_requests/2) in futures-retry.
* [ ] decide for the name of this crate.
* [ ] Create proper tests to check this crate working as expected.
* [ ] add CI and run tests on multiplatform.
