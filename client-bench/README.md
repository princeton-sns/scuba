# Running Benchmarks

## Generating data

To run password manager benchmarks, run `cargo run -- pass`. This should create
an `update_pass_output_[num]` directory, where `num` monotonically increases if
there already exists an `update_pass_output_[num]` directory (the highest `num`
corresponds to the most recent benchmark results).

To run family app benchmarks, run `cargo run -- fam`.

## Aggregating and visualizing data

### Setup

Create a python virtual environment. I am using python 3.10.12 so I can create
a virtual environment using:

```sh
python3 -m venv venv
```

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

### parse_results.ipynb

Before running check that all values in the second cell are correct. Then running
all cells should just work (expects num clients to be 1, 2, 4, 8, 16, and 32).

### gen_figures.ipynb

TODO
