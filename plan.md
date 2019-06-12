# Serverless Done Good


* I am the 90% use case
* I don't want lots of options for API design. I want one simple, well supported way which WORKS
* And by WORKS I mean is SIMPLE and TYPESAFE and therefore HARD TO SCREW UP
* I define my interface with OpenAPI3 (possibly a constrained version of it, since it has too many knobs)
* I use a tool to codegen a trait.
* It codegens a client too.
* I implement this trait. In doing so I have fulfilled the API interface.
* I upload the code to the platform. The platform will validate and compile the code (or else it will barf).
* The code is now deployed. The endpoints are live at a given IP
* I am charged based on how many seconds of compute I use (I can set timeouts)
* If I write async code, I am not charged for non-blocking IO (i.e. database lookups)
* If I want to use a websocket, I will


## Stuff that I don't care about that is handled for me

* HTTPS
* Auto-scaling
* Authentication
* Errors (if my code crashes, the endpoint returns `500` and keeps going)
* Logging
* Metrics/Alerts
* Swagger UI is auto-hosted

## State

* Obviously most APIs have some kind of state, if only a list of users.
* State should be held in a database.
* So, can we have 'database done right'?
* Definitely a tricker problem. Aurora is probably the model
* 'Postgres, but without the bother'
