# Petstore Demo

* The API is specified in `petstore.yaml`.
* The API is generated in `build.rs`.
* The server implementation is `src/lib.rs`.

There are two binaries. `cargo run --bin petstore-server` launches the server,
`cargo run --bin petstore-client` launches a client and tests various server endpoints.

Execute `./test.sh` to run a demo.

Be sure to run `cargo doc --open` to inspect the generated code!

(Note that the server code has been programmed to randomly fail one-in-twenty times.
Just like a real server!)
