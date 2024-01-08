# How to run

Spin up two clients in separate windows by running `cargo run` in each window.
When the apps start, you can type `help` to see what commands are available to you.
You can quit the app via `Ctrl-C`.
Below is a demo that shows one way to use the app.

## Demo

Run the following commands, where client A and client B are the two clients you just spun up (order doesn't matter).

| Client A Cmds | Retvals | Client B Cmds | Retvals |
| :--- | :--- | :--- | :--- |
| `init_new_device` | | `init_new_device` | |
| | | `get_idkey` | `<idkey-B>` |
| `add_contact <idkey-B>` | | | |
| `config_app_password -a netflix -l 8 -n --lc --uc -s` | `<config-id>` | | |
| `add_password -a netflix -s abc -t hotp -u nataliepopescu` | `<pass-id>` | | |
| | | `get_name` | `<name-B>` |
| `share -c <config-id> -p <pass-id> -w <name-B>` | | | |
| | | `get_data <pass-id>` | `shared pass obj (counter=0)` |
| | | `get_otp <pass-id>` | `<otp>` |
| `get_data <pass-id>` | `updated pass obj (counter=1)` | `get_data <pass-id>` | `updated pass obj (counter=1)` |
