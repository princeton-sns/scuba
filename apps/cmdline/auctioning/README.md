# How to run

An application client can be spun up via `cargo run`.
When the app starts, you can type `help` to see what commands are available to you.
You can quit the app via `Ctrl-D`.
Below is a demo that shows one way to use the app.

## Demo

Spin up two clients in separate windows by running `cargo run` in each window.
Then, run the following commands, where client A and client B are the two clients you just spun up (order doesn't matter).

### Open auction

| Client A Cmds | Retvals | Client B Cmds | Retvals |
| :--- | :--- | :--- | :--- |
| `init_new_device` | | `init_new_device` | |
| | | `get_idkey` | `<idkey-B>` |
| `add_contact <idkey-B>` | | | |
| | | `get_name` | `<name-B>` |
| `add_to_open_auction_list <name-B>` | | | |
| `create_auction -d "book" -b 3 --start-date <todays-date> --start-time <soon> --end-date <todays-date> --end-time <soon+5min>` | `<auc-id>` | | |
| `announce_open_auction <auc-id>` | | | |
| | | `get_data <auc-id>` | `auction obj (highest bid=3)` |
| | | `submit_bid -i <auc-id> -b 5` | `<bid-id>` |
| `get_name` | `<name-A>` | | |
| | | `share -i <bid-id> -r <name-A>` | |
| | | `get_data <auc-id>` | `auction obj (highest bid=3)` |
| `(before start) apply_bid -i <bid-id>` | `"TIME TOO EARLY\n Invalid bid."` | | |
| `(after start) apply_bid -i <bid-id>` | `"Bid submitted!"` | | |
| | | `get_data <auc-id>` | `updated auction obj (highest bid=5)` |
| `(before end) announce_sale -i <auc-id>` | `"TIME TOO EARLY\n Auction is not over, cannot announce sale."` | | |
| `(after end) announce_sale -i <auc-id>` | `"Auction is over, sold to client Some(<name-B>) for $Some(5)"` | | |

### Closed auction

| Client A Cmds | Retvals | Client B Cmds | Retvals |
| :--- | :--- | :--- | :--- |
| `init_new_device` | | `init_new_device` | |
| | | `get_idkey` | `<idkey-B>` |
| `add_contact <idkey-B>` | | | |
| `create_auction -d "book" -b 3 --start-date <todays-date> --start-time <soon> --end-date <todays-date> --end-time <soon+5min>` | `<auc-id>` | | |
| | | `get_name` | `<name-B>` |
| `share -i <auc-id> -r <name-B>` | | | |
| | | `get_data <auc-id>` | `auction obj (highest bid=3)` |
| | | `submit_bid -i <auc-id> -b 5` | `<bid-id>` |
| `get_name` | `<name-A>` | | |
| | | `share -i <bid-id> -r <name-A>` | |
| | | `get_data <auc-id>` | `auction obj (highest bid=3)` |
| `(before start) apply_bid -i <bid-id>` | `"TIME TOO EARLY\n Invalid bid."` | | |
| `(after start) apply_bid -i <bid-id>` | `"Bid submitted!"` | | |
| | | `get_data <auc-id>` | `updated auction obj (highest bid=5)` |
| `(before end) announce_sale -i <auc-id>` | `"TIME TOO EARLY\n Auction is not over, cannot announce sale."` | | |
| `(after end) announce_sale -i <auc-id>` | `"Auction is over, sold to client Some(<name-B>) for $Some(5)"` | | |

