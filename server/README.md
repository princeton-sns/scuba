# Run commands

Before running, make sure the `epoch_log.txt` file nor `persist-outbox` directory
exist, removing them if they do via:

```sh
rm epoch_log.txt && rm -rf persist-outbox/
```

To start the sequencer, run: 

```sh
cargo run --release sequencer --port 8082 --shard-count 1
```

To start the shards, run:

```sh
cargo run --release shard --port 8081 --public-url "http://localhost:8081" --sequencer-url "http://localhost:8082" --inbox-count 1 --outbox-count 1
```
