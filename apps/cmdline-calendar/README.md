# Running the Calendering App

## Server setup

```sh
git clone git@github.com:princeton-sns/sharded-noise-server.git
```
```sh
cd sharded-noise-server/parallel-networked-server
```

In one window run the sequencer: 

```sh
rm -rf epoch_log.txt && rm -rf persist-outbox/ && cargo run --release sequencer --port 8082 --shard-count 1
```

and in another start the shards: 

```sh
cargo run --release shard --port 8081 --public-url "http://localhost:8081" --sequencer-url "http://localhost:8082" --inbox-count 1 --outbox-count 1
```

## Client demo

Spin up two clients by running

```sh
cargo run
```

in two windows. Then run the following commands, where client A and client B are the two clients you just spun up.

| Client A Cmds | Retvals | Client B Cmds | Retvals |
| :--- | :--- | :--- | :--- |
| `init_new_device` | | `init_new_device` | |
| `init_role --provider` | | `init_role --patient` | |
| | | `get_idkey` | `<idkey-B>` |
| `add_patient <idkey-B>` | | |
| | | `get_name` | `<name-B>` |
| `share_availability <name-B>` | | |
| `get_name` | `<name-A>` | | |
| | | `request_appointment -p <name-A> -d YYYY-MM-DD -t HH:MM:SS` | |
| `get_data` | should see 3 datums, one of which is the pending appointment; copy the `<appt-id>` | | |
| `confirm_appointment <appt-id>` | no messages sent; expect 1) appointment.pending=false and 2) updated availability object, both on both clients A and B | | |











