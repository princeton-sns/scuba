# How to run

An application client can be spun up via `cargo run`.
When the app starts, you can type `help` to see what commands are available to you.
You can quit the app via `Ctrl-D`.
Below is a demo that shows one way to use the app.

## Demo

Spin up two clients in separate windows by running `cargo run` in each window.
Then, run the following commands, where client A and client B are the two clients you just spun up (order doesn't matter).

| Client A Cmds | Retvals | Client B Cmds | Retvals |
| :--- | :--- | :--- | :--- |
| `init_new_device` | | `init_new_device` | |
| `init_provider_role` | | `init_patient_role` | |
| | | `get_idkey` | `<idkey-B>` |
| `add_patient <idkey-B>` | | | |
| | | `get_name` | `<name-B>` |
| `share_availability <name-B>` | | | |
| `get_name` | `<name-A>` | | |
| | | `request_appointment -p <name-A> -d YYYY-MM-DD -t HH:MM:SS` | `<appt-id>` |
| | | `get_data` | `pending appt obj + empty avail obj` |
| `confim_appointment <appt-id>` | | | |
| | | `get_data` | `confirmed appt obj + avail obj with one occupied slot` |
