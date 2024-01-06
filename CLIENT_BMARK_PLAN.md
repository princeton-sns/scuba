# Client Benchmarks

## Storage

- [ ] Each client's own keys
  - [ ] identity key pair (both signing + encryption) -- pickled
    - [x] Public: 164 B (246 B base64)
    - [ ] Private: ?
  - [ ] Curve25519 one-time key pairs -- pickled
    - [x] Public: 164 B (246 B base64)
    - [ ] Private: ?
  - From OLM C++ implement, each Curve25519 (public/private) key should be 32 bytes
- [ ] Each pairwise session: ? depends on the number of messages sent/received
  - 352 B in base64 (234.66667 B) -- not right
  - 438 B in base64 (292 B)
  - 523 B in base64 (348.66667 B) -- not right
  - 630 B ...
  - 715 B ...
  - TODO -- does binary encoding fix this? no, session sizes still change
- [x] POVS history + attestations equation
- [ ] POVS/att scenario 1
  - [x] estimate
  - [ ] confirmation
- [ ] POVS/att scenario 2
  - [x] estimate
  - [ ] confirmation
- [ ] POVS/att scenario 3 (extreme; e.g. open auction)
  - [x] estimate
  - [ ] confirmation

## Bandwidth

- [x] Per-recipient payload overhead
  - [x] Symmetric key:
    - plaintext = 16 B
  - [x] Validation payload: 40 B
    - [x] head: 32 B
    - [x] index: 8 B
  - [x] Recipient lists
    - Each recipient pub idkey = 43 B (diff format from above)
    - Each recipient list = #rcpts x idkey size
- [x] Constant encryption overhead (over plaintext)
- [x] Range of common ciphertext sizes

- [ ] Another bandwidth example/setup w many recipients and large op size
- [ ] Figure out why add-contact has a diff per-rcpt bandwidth

### Encryption overhead numbers

 - [x] (op-pt-bytes, common-pt-bytes) => recipient list overhead
 - [x] (common-pt-bytes, common-ct-bytes) => symmetric encryption overhead
 - [x] (op-pt-bytes, common-ct-bytes) => recipient list + symmetric encryption overhead
 - [x] (0, per-recipient-pt-bytes) => val/encryption metadata overhead
 - [x] (per-recipient-pt-bytes, per-recipient-ct-bytes) => asymmetric encryption overhead
   - generally 212 (pt bytelen = 43)
   - except for add_contact op => (pt bytelen = 3, while ct bytelen = 267)
   - why is this larger?

## Computation

Recalling findings from the previous submissions, clients microbenchmarks are not
particularly useful or interesting for the following reasons: sender-side POVS 
essentially just performs a few copies for each recipients, sender-side encryption
just performs X amount of encryption, receiver-side decryption just performs 2
decrypt operations, and receiver-side POVS just compares a couple values and calculates
a hash. 

## Computation Revised

Stacked latency bar graphs
- app?
- dal
- POVS
- encryption

2-3 application: passmanager, socialmedia, auction
- if 2 apps, 3 ops
- if 3 apps, 2 ops

- start with 2 and 2

per app: 2-3 representative operations (should have variable message sizes)
- passmanager: update_password, get_otp, share_password?
- socialmedia: set_post, share_location?

for each operation, vary number of recipients















