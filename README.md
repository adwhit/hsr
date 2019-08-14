# HSR

Build fast HTTP apis, fast, with Rust + OpenAPI.

 * Define your API as an [OpenAPI 3.0](https://swagger.io/docs/specification/about/) spec
 * Code-gen a server/client interface (based on actix-web)
 * Implement your interface (simple, safe, strongly typed!)
 * Run! 'If it compiles, it works'


## Quickstart

Take a look at the [quickstart repo](examples/quickstart). It contains the
minimum boilerplate needed to get started.

## Less quick start

Take a look at the [petstore repo](examples/petstore) for a more in-depth example.

## Roadmap

- [x] HTTP server
- [x] HTTP client
- [x] Type-safe path/query/json handling
- [x] High performance
- [ ] HTTPS
- [ ] Benchmarks
- [ ] Return content-types other than JSON
- [ ] AnyOf/OneOf schema support
- [ ] Support all features necessary to get `petstore-expanded.yaml` working
- [ ] Support headers
- [ ] (Once the futures ecosystem has matured) - move away from futures v1
- [ ] Option to use existential types
- [ ] Advanced server configuration (with middleware etc)
- [ ] Even less boilerplate
- [ ] Works on stable Rust
- [ ] JSON schema
- [ ] Tutorial

## FAQ

**What's the difference between this and [swagger-rs](https://github.com/Metaswitch/swagger-rs)?**

* `swagger-rs` is a more mature projuect with institutional backing.
* It uses `futures v0.1` throughout - `hsr` attempts to use `futures v0.3` to allow `async/await` syntax
  -  Consequently - it works on stable, `hsr` requires nightly
* Requires the codegenerator, written in Java - `hsr` is 100% Rust
  - That means that the `swagger-rs` code generator is likely much more correct...
  - ..but the `hsr` build process is much more seamlessly integrated into typical Rust workflow
* Error-handling story somewhat different
* Surely lots of other differences

## License

MIT
