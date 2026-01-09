#!/usr/bin/env python3
"""Example demonstrating external Python package usage in Buck2.

This example uses the `click` package for CLI building.
To make this work with Buck2, you need a third-party setup.
"""

import click

@click.command()
@click.option("--name", default="World", help="Name to greet")
def main(name: str) -> None:
    click.echo(f"Hello, {name}!")

if __name__ == "__main__":
    main()
