# Ulam Leapers

"Ulam Leapers" is a term coined by this project for a family of
mathematical constructs described by [Jonas Karlsson](https://jonka364.github.io/)
at https://jonka364.github.io/stendhal/stendhal.html. 

This project is interested in simulation of such mathematical constructs - generating
a 2-dimensional grid representation.

The scope of this project:

- a library crate providing necessary infrastructure for creating, running, saving, and loading
simulations of such mathematical constructs, with generous constraints on the 
number of players, pieces, and player relations.
- a binary crate with an [egui](https://github.com/emilk/egui) GUI application
allowing easy creation and visualization of simulations
- a specification for a persistent binary format [ULS](docs/uls) used to store simulations 