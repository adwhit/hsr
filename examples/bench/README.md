## Benchmark

This benchmark features just endpoints, a one basic `GET` and a basic `POST`.

To run, you will need to install `wrk2`

``` sh
# start the server
cargo run --release
```
Separate terminal:

``` sh
# get benchmark
wrk -c10 -d10 -t4 -R 130000 http://localhost:8000/bench

# post benchmark
wrk -c10 -d10 -t4 -R 130000 -s POST.lua http://localhost:8000/bench
```
You may need to adjust `-R <rate>` until you max out your cpus.


## Results


Get:
``` sh
➜ wrk -c10 -d10 -t4 -R 130000 http://localhost:8000/bench
Running 10s test @ http://localhost:8000/bench
  4 threads and 10 connections
  Thread Stats   Avg      Stdev     Max   +/- Stdev
    Latency   294.43ms  233.42ms 809.47ms   58.61%
    Req/Sec       -nan      -nan   0.00      0.00%
  1196889 requests in 10.00s, 85.61MB read
Requests/sec: 119694.82
Transfer/sec:      8.56MB
```

Post:

``` sh
➜ wrk -c10 -d10 -t4 -R 130000 -s POST.lua http://localhost:8000/bench
Running 10s test @ http://localhost:8000/bench
  4 threads and 10 connections
  Thread Stats   Avg      Stdev     Max   +/- Stdev
    Latency   943.52ms  612.25ms   2.14s    57.49%
    Req/Sec       -nan      -nan   0.00      0.00%
  1024410 requests in 10.00s, 133.84MB read
Requests/sec: 102446.34
Transfer/sec:     13.38MB

```
