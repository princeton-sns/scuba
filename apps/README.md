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
