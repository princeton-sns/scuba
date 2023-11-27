# Client Benchmarks

## Storage

- [ ] Each client's own keys: ?
  - [ ] Ed25519 fingerprint pair (exclude?)
    - Public: ?
    - Private: ?
  - [ ] Curve25519 identity pair
    - Public: 48 B?
    - Private: ?
  - [ ] Curve25519 one-time pairs
    - Public: ?
    - Private: ?
- [ ] Each pairwise session: ?
- [x] POVS history + attestations equation
- [ ] POVS/att scenario 1
  - estimate: ?
  - confirmation: ?
- [ ] POVS/att scenario 2
  - estimate: ?
  - confirmation: ?
- [ ] POVS/att scenario 3 (extreme; e.g. open auction)
  - estimate: ?
  - confirmation: ?

## Bandwidth

- [ ] Per-recipient payload overhead
  - [x] Symmetric key:
    - plaintext = 16 B
  - [x] Validation payload: 40 B
    - [x] head: 32 B
    - [x] index: 8 B
  - [ ] Recipient lists
    - Each recipient pub idkey = ? (from above)
    - Each recipient list = #rcpts x idkey size
- [ ] Constant encryption overhead (over plaintext)
- [ ] Common ciphertext sizes?

## Computation

- [ ] Sending overhead (#recipients + size of op)
  - POVS
  - Encryption
- [ ] Receiving overhead (#recipients + size of op)
  - Store attestation?
  - Decryption
  - POVS
