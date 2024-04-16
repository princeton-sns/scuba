# Running Benchmarks

## Setting up cloudlab experiment

1. Reserve a single `m400` cloudlab machine (TODO need extra storage?)
1. `git clone` this repository in the machine's home directory.
1. Install Rust: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh` and then run `. "$HOME/.cargo/env"`
1. Install `tmux`
   ```sh
   sudo apt update
   sudo apt upgrade (?)
   sudo apt install tmux
   tmux new
   ```
1. Install `pkg-config`: `sudo apt install pkg-config`
1. Install `libssl-dev`: `sudo apt install libssl-dev`
1. Install `cmake`: `sudo apt install cmake`
1. Start the SCUBA server via [these](https://github.com/princeton-sns/scuba/blob/main/server/README.md) instructions
1. Then follow the instructions below to run the client benchmarks.
1. When the benchmarks have finished running, recursively `scp` the results over to your local computer so you can run the jupyter notebooks on them. 

## Generating data

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

Create a python virtual environment. I am using python 3.7.17 so I can create
a virtual environment using:

```sh
python3.7 -m venv venv
```

Note python 3.8 and 3.10 both pose different problems for tikzplotlib used in `gen_figures.ipynb`, so try to use 3.7.
The virtual environment can then be activated by:

```sh
source venv/bin/activate
```

which puts the `venv` path at the beginning of your PATH. Then install jupyter notebook
inside the virtual environment:

```sh
pip install notebook
```

And add a virtual environment as a jupyter notebook kernel as well:

```sh
python -m ipykernel install --user --name=venv
```

Then run jupyter notebook via:

```sh
jupyter notebook
```

You will need to install matplotlib, pandas, and tikzplotlib via pip in order for the notebooks to run:

```sh
pip install [package]
```

TODO create requirements.txt (`pipreqs` isn't working and `pip freeze` outputs too much, maybe try `pipenv`).

### Running `parse_results.ipynb`

**Before running check that all values in the second cell are correct.** Then running
all cells should just work (expects num clients to be 1, 2, 4, 8, 16, and 32).

This generates two output files for each benchmark category parsed: `send_means_*.txt` and `recv_means_*.txt`. These files are then used by `gen_figures.ipynb` to generate stacked bar graphs. Note that `gen_figures.ipynb` expects parsed results for *both* benchmark categories so the resulting bar graphs can depict both categories side-by-side.

### Running `gen_figures.ipynb`

**Before running check that all values in the second cell are correct.** Then running all cells should just work. The notebook will output two `*_bargraph.tikz.tex` files, one for send-path latencies and one for receive-path latencies.

### Last steps

Copy over the generated files into the appropriate paper directory.
