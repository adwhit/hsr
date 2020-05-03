# Changelog

## Release 0.3.0

* Huuuge refactor to allow more precise type definitions.
   - AllOf, AnyOf, OneOf now work
   - All the generated names are better
   - 'title' is supported
   - Much better error checking

* Added more comprehensive set of tests

* Added a basic tutorial

## Release 0.2.0

* Convert to `actix 2.0` and `async/await`

* Added support for HTTP verbs other than GET and POST

* Added HTTPS support

* Added simple benchmark example

* `serve` function takes `hsr::Config` rather than just `Url`

## Release 0.1.0 - 2019-08-14

* First public release
