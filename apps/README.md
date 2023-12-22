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

Each application client can be spun up by running `cargo run` in the relevant subdirectory. See the README in each application's subdirectory for specific instructions on how to interact with each application.

# Apps Status

## Auctioning

- [x] compiles

- Implements both open and closed auctions
- Each auction is organized and decided by an auctioneer client
- In open auctions, clients must be added to the open auction list (via `add_to_open_auction_list`)
- Uses single-key DAL to achieve sequential consistency

## Calendar

- [x] compiles

- Patient clients can request specific time slots from provider clients
- Providers share their overall availability with all patients
- Providers either confirm or deny requested appointments
- When a provider confirms an appointment, they also atomically update their availability object for all patients
- Should use transactional DAL to achieve serializability (currently using single-key DAL)

## Chess

- [ ] compiles

## Lightswitch

- [ ] compiles

## Password Manager

- [x] compiles

- Implements both TOTP and HOTP
- Passwords are stored along with other metadata (application and user names)
- Passwords can also be automatically generated
- Uses single-key DAL to achieve linearizability

## Period Tracker

- [ ] compiles

## Family Social Media

- [x] compiles

- Family members can interact within "families"
- They can create posts, comment on posts, and share their locations
- Uses single-key DAL to achieve causal consistency
