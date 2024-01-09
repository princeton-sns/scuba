# How to run

An application client can be spun up via `cargo run`.
When the app starts, you can type `help` to see what commands are available to you.
You can quit the app via `Ctrl-D`.
Below is a demo that shows one way to use the app.

## Demo

Spin up two clients in separate windows by running `cargo run` in each window.
Then, run the following commands, where client A and client B are the two clients you just spun up (order doesn't matter).

### Posting and commenting

| Client A Cmds | Retvals | Client B Cmds | Retvals |
| :--- | :--- | :--- | :--- |
| `init_new_device` | | `init_new_device` | |
| | | `get_idkey` | `<idkey-B>` |
| `add_contact <idkey-B>` | | | |
| `init_family` | `<fam-id>` | | |
| | | `get_name` | `<name-B>` |
| `add_to_family -f <fam-id> -c <name-B>` | | | |
| | | `get_data <fam-id>` | `shared fam obj` |
| | | `post_to_family -f <fam-id> -p "hello fam!"` | `<post-id>` |
| `get_data <post-id>` | `B's post` | | |
| `comment_on_post -p <post-id> -c "hello back!"` | `<comment-id>` | | |
| | | `get_data <comment-id>` | `A's comment on B's post` |

### Sharing and updating locations

| Client A Cmds | Retvals | Client B Cmds | Retvals |
| :--- | :--- | :--- | :--- |
| `init_new_device` | | `init_new_device` | |
| | | `get_idkey` | `<idkey-B>` |
| `add_contact <idkey-B>` | | | |
| `init_family` | `<fam-id>` | | |
| | | `get_name` | `<name-B>` |
| `add_to_family -f <fam-id> -c <name-B>` | | | |
| | | `get_data <fam-id>` | `shared fam obj` |
| `get_location_id` | `<loc-id-A>` | | |
| `share_location -m <name-B>` | | | |
| | | `get_data <loc-id-A>` | `A's location obj (x=1.0)` |
| `update_location` | | | |
| | | `get_data <loc-id-A>` | `A's updated location obj (x=2.0)` |

