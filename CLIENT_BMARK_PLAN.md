# Client Benchmarks

## Storage

- [ ] Each client's own encryption keys
  - RES
- [ ] Each pairwise session
  - RES
- [ ] POVS history + attestations equation
  - RES
- [ ] POVS/att scenario 1
  - estimate: RES
  - confirmation: RES
- [ ] POVS/att scenario 2
  - estimate: RES
  - confirmation: RES
- [ ] POVS/att scenario 3 (extreme; e.g. open auction)
  - estimate: RES
  - confirmation: RES

## Bandwidth

- [ ] Constant encryption overhead
- [ ] Common ciphertext sizes?
- [ ] Per-recipient payload overhead
  - [ ] Symmetric key: RES
  - [ ] Validation payload: RES
    - [ ] head: RES
    - [ ] index: RES
    - [ ] recipient list scenario 1 (1 rcpt): RES
    - [ ] recipient list scenario 2 (8 rcpt): RES
    - [ ] recipient list scenario 3 (64 rcpt): RES

## Computation

- [ ] Sending overhead (#recipients + size of op)
  - POVS
  - Encryption
- [ ] Receiving overhead (#recipients + size of op)
  - Store attestation?
  - Decryption
  - POVS
