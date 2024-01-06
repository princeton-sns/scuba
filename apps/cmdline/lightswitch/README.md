# cmdline-lightswitch

## Try it yourself!

### Run local server

```sh
git clone https://github.com/princeton-sns/frida/tree/rust-client
cd frida/server
go run frida_server.go
```

### Run client

```sh
cargo run
```

### Example commands

| Client A Cmds | Retvals | Client B Cmds | Retvals |
| :--- | :--- | :--- | :--- |
| > `create_device` | | > `create_device` | |
| > `get_idkey` | `sk7...` | | |
| | | > `add_contact sk7...` | |
| > `get_name` | `a` | > `get_name` | `b` |
| > `get_contacts` | `b` | > `get_contacts` | `a` |
| > `add_lightbulb` | `lightbulb/h34...` | | |
| > `share_lightbulb -i lightbulb/h34... -n b` | | |
| > `get_lightbulb_state lightbulb/h34...` | `id: ..., group: ..., val: { "is_on": false }` | > `get_lightbulb_state lightbulb/h34...` | `id: ..., group: ..., val: { "is_on": false }` |
| > `turn_on lightbulb/h34...` | | | |
| > `get_lightbulb_state lightbulb/h34...` | `id: ..., group: ..., val: { "is_on": true }` | > `get_lightbulb_state lightbulb/h34...` | `id: ..., group: ..., val: { "is_on": true }` |
| | | > `turn_off lightbulb/h34...` | |
| > `get_lightbulb_state lightbulb/h34...` | `id: ..., group: ..., val: { "is_on": false }` | > `get_lightbulb_state lightbulb/h34...` | `id: ..., group: ..., val: { "is_on": false }` |

