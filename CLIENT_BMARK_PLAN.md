# Client Benchmarks

## Storage

- [ ] Each client's own keys
  - [ ] identity key pair (both signing + encryption) -- pickled
    - [x] Public: 164 B (246 B base64)
    - [ ] Private: ?
  - [ ] Curve25519 one-time key pairs -- pickled
    - [x] Public: 164 B (246 B base64)
    - [ ] Private: ?
- [ ] Each pairwise session: ? depends on the number of messages sent/received
  - 352 B in base64 (234.66667 B) -- not right
  - 438 B in base64 (292 B)
  - 523 B in base64 (348.66667 B) -- not right
  - 630 B ...
  - 715 B ...
  - TODO -- does binary encoding fix this?
- [x] POVS history + attestations equation
- [ ] POVS/att scenario 1
  - [x] estimate: ?
  - [ ] confirmation: ?
- [ ] POVS/att scenario 2
  - [x] estimate: ?
  - [ ] confirmation: ?
- [ ] POVS/att scenario 3 (extreme; e.g. open auction)
  - [x] estimate: ?
  - [ ] confirmation: ?

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
- [ ] Constant encryption overhead (over plaintext)
- [ ] Range of common ciphertext sizes

## Computation

- [ ] Sending overhead (#recipients + size of op)
  - POVS
  - Encryption
- [ ] Receiving overhead (#recipients + size of op)
  - Store attestation?
  - Decryption
  - POVS
