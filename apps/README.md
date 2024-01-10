# How to Run One or More Apps

## Start the SCUBA server

In two separate windows, navigate to the top-level server directory:

```sh
cd ../server
```

In one window, start the sequencer: 

```sh
rm -rf epoch_log.txt && rm -rf persist-outbox/ && cargo run --release sequencer --port 8082 --shard-count 1
```

In the other window, start the shards:

```sh
cargo run --release shard --port 8081 --public-url "http://localhost:8081" --sequencer-url "http://localhost:8082" --inbox-count 1 --outbox-count 1
```

Once those commands are successful, the server is up and running.

## Run each client app

Each application client can be spun up by running `cargo run` in the relevant subdirectory. See the README in each application's subdirectory for specific instructions on how to interact with each application. In general, once a client is spun up, typing `help` will print out the available application commands.

# Apps Status

## Auctioning

- [x] compiles
- [x] runs

- Implements both open and closed auctions
- Each auction is organized and decided by an auctioneer client
- In open auctions, clients must be added to the open auction list (via `add_to_open_auction_list`)
- Uses single-key TANK to achieve **sequential consistency** and unique correctness properties that can catch cheating

## Calendar

- [x] compiles
- [x] runs

- Patient clients can request specific time slots from provider clients
- Providers share their overall availability with all patients
- Providers either confirm or deny requested appointments
- When a provider confirms an appointment, they also atomically update their availability object for all patients
- TODO Should use transactional TANK to achieve **serializability** (currently using single-key TANK)

## Family Social Media

- [x] compiles
- [x] runs

- Family members can interact within "families"
- They can create posts, comment on posts, and share their locations
- Uses single-key TANK to achieve **causal consistency**

## Password Manager

- [x] compiles
- [x] runs

- Implements both TOTP and HOTP
- Passwords are stored along with other metadata (application and user names)
- Passwords can also be automatically generated
- Uses single-key TANK to achieve **linearizability**

## Chess

- [ ] compiles
- [ ] runs

## Lightswitch

- [ ] compiles
- [ ] runs

## Period Tracker

- [ ] compiles
- [ ] runs

