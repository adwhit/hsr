# HSR

Build fast HTTP apis fast, with Rust, and [OpenAPI](https://swagger.io/docs/specification/about/)

 * Define your API as an [OpenAPI 3.0](https://github.com/OAI/OpenAPI-Specification) spec
 * HSR will code-gen a server/client interface
 * Implement your interface (simple, safe, strongly typed!)
 * Run! 'If it compiles, it works!'


## Quickstart

Take a look at the [quickstart example](examples/quickstart). It contains the
minimum boilerplate needed to get up and running.

## Less Quick Start

Take a look at the [petstore example](examples/petstore) for a more in-depth example
showcasing a 'database' backend.

## Features

- [x] HTTP server
- [x] HTTP client
- [x] Type-safe path/query/json handling
- [x] Supports all HTTP verbs
- [x] High performance
- [x] Based on `async/await` and `actix-web 2.0`
- [x] Use async in trait (partial - uses the amazing [`async-trait`](https://github.com/dtolnay/async-trait))
- [x] Works on stable Rust
- [x] Benchmarks
- [x] Full test spec

## Roadmap

- [ ] HTTPS
- [ ] Support headers
- [ ] Support default values
- [ ] Advanced server configuration (with middleware etc)
- [ ] support JSON (not just YAML) schema
- [ ] Tutorial
- [ ] Return content-types other than JSON
- [ ] Auto-generate a client binary

## FAQ

**What's the difference between this and [swagger-rs](https://github.com/Metaswitch/swagger-rs)?**

I haven't used `swagger-rs`, however the major difference is that `hsr` is pure Rust,
whereas `swagger-rs` takes advantage of an existing code-generator written in Java.
That means that the `swagger-rs` is more mature likely much more correct,
`hsr` is much easier to use and is seamlessly integrated into typical Rust workflow.

**What do you mean, 'fast'?**

It uses [Actix-Web](https://github.com/actix/actix-web) under the hood, rated as one of the
fastest web frameworks by [techempower](https://www.techempower.com/benchmarks/#section=data-r18&hw=ph&test=fortune).
`hsr` imposes very little overhead on top.

As a simple and not-very-scientific benchmark, on my laptop (X1 Carbon 6th Gen)
I measured around:

* 120,000 requests/second for an empty GET request
* 100,000 requests/second for a POST request with a JSON roundtrip

Try it yourself! See the [bench example](/examples/bench).

**Why the name?**

I like fast trains.

## License

MIT
