# How to run (demo)

Spin up two clients in separate windows by running `cargo run` in each window.
Then run the following commands, where client A and client B are the two clients you just spun up (order doesn't matter).

| Client A Cmds | Retvals | Client B Cmds | Retvals |
| :--- | :--- | :--- | :--- |
| `init_new_device` | | `init_new_device` | |
| | | `get_idkey` | `<idkey-B>` |
| | | `get_name` | `<name-B>` |
| `add_contact <idkey-B>` | | | |
| `config_app_password -a <app-name> -l <len> -n --lc --uc -s` | `<config-id>` | | |
| `add_password -a <app-name> -s abc -t hotp -u <uname>` | `<pass-id>` | | |
| `share -c <config-id> -p <pass-id> -w <name-B>` | | | |
| | | `get_data` | `shared pass obj (counter=0)` |
| | | `get_otp <pass-id>` | `OTP` |
| `get_data` | `updated pass obj (counter=1)` | `get_data` | `updated pass obj (counter=1)` |
