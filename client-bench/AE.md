# Running Client Benchmarks

## Setting up Cloudlab experiment

1. Reserve a single `m400` Cloudlab machine (no temporary file system needed) -
   this hardware is located in the Utah cluster. Use the `small-lan` profile,
   and default OS (or Ubuntu 22). A few hours should be sufficient but you can 
   reserve the machine for 10 to be safe, and terminate if you are done early. 
1. SSH into the node with the following flags: `-A -L
   localhost:8888:localhost:8888` so you can clone the git repo via SSH and
   preview jupyter notebook results in your local browser. You will also likely 
   need to set up an SSH key for this. In case you haven't already, follow the
   instructions listed here: 
   https://docs.github.com/en/authentication/connecting-to-github-with-ssh/generating-a-new-ssh-key-and-adding-it-to-the-ssh-agent
1. Run `sudo apt update`
1. Run `sudo add-apt-repository ppa:deadsnakes/ppa`
1. `sudo apt install -y tmux pkg-config libssl-dev cmake vim python3.7 python3.7-venv`
1. Install Rust (using the default installation options): `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh` and then run `. "$HOME/.cargo/env"`
1. `git clone` the SCUBA reposity: https://github.com/princeton-sns/scuba
1. `cd scuba && git checkout bench-cleanup`
1. `tmux new` or another terminal session manager of your choice. 
1. `cd server` and start the SCUBA server via [these](https://github.com/princeton-sns/scuba/blob/main/server/README.md) instructions (each of those commands should be run in a separate window/tab/pane, and always start the sequencer before starting the shards). 
1. Then follow the instructions below to run the client benchmarks.

## Generating data

```sh
cd scuba/client-bench
```

### Password Manager (`update_password` function)

To run password manager benchmarks, run:

```sh
cargo run --release -- pass
```

from the `client-bench` directory. This should create
an `update_pass_output_[num]` directory, where `num` monotonically increases if
there already exists an `update_pass_output_[num]` directory (the highest `num`
corresponds to the most recent benchmark results).

### Family Social Media (`edit_post` function)

To run family app benchmarks, run:

```sh
cargo run --release -- fam
```

from the `client-bench` directory. This should create
an `edit_post_output_[num]` directory, where `num` monotonically increases if
there already exists an `edit_post_output_[num]` directory (the highest `num`
corresponds to the most recent benchmark results).

## Aggregating and visualizing data

### Setup

Create a python virtual environment (using your installed python3.7 tools). You
can run the following command in the top-level `scuba` directory:

```sh
python3.7 -m venv venv
```

The virtual environment can then be activated by:

```sh
source venv/bin/activate
```

which puts the `venv` path at the beginning of your PATH. Then install jupyter notebook
inside the virtual environment:

```sh
pip install notebook matplotlib pandas tikzplotlib
```

And add a virtual environment as a jupyter notebook kernel as well:

```sh
python -m ipykernel install --user --name=venv
```

Then run jupyter notebook via:

```sh
jupyter notebook
```

### Running `parse_results.ipynb`

Paste the outputted link in your browser, navigate to `client_bench/parse_results.ipynb`, and select "Kernel > Restart & Run All".

Once that finishes running, navigate to the top of that notebook to the second
cell and toggle the value of `pass_bmark`. For instance, if you see `pass_bmark
= False`, set `pass_bmark = True` and select "Kernel > Restart & Run All" again
(or vice versa if `pass_bmark = True` already). This ensures that results are
parsed for both the password manager benchmark and the family social media app
benchmark. If you forget this, running the `gen_figures.ipynb` notebook will 
give you an error saying a "parsed_send" or "parsed_recv" file doesn't exist in
one of the generated directories. 

### Running `gen_figures.ipynb`

Simply open `client_bench/gen_figures.ipynb" and select "Kernel > Restart & Run
All". 

The last two figures should closely match those in Figure 10 of the paper. 

