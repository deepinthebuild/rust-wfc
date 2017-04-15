#![allow(dead_code)]

use utils;

use bit_vec::BitVec;
use sourceimage::{RGB, SeedImage};
use ndarray::prelude::*;

use std::collections::HashMap;
use std::cell::RefCell;
use std::{f64, usize};


#[derive(Debug)]
struct UncertainCell {
    possible_colors: RefCell<BitVec>,
    possible_states: RefCell<BitVec>,
}

impl UncertainCell {
    pub fn new(num_colors: usize, num_states: usize) -> UncertainCell {
        let possible_colors = RefCell::new(BitVec::from_elem(num_colors, true));
        let possible_states = RefCell::new(BitVec::from_elem(num_states, true));
        UncertainCell {
            possible_colors: possible_colors,
            possible_states: possible_states,
        }
    }

    #[inline(always)]
    pub fn valid_color(&self, palette_index: usize) -> bool {
        self.possible_colors.borrow().get(palette_index).expect("Index out of range!")
    }

    pub fn entropy<T>(&self, concrete_states: &[(T, usize)]) -> Option<f64> {
        let possible_states = self.possible_states.borrow();
        debug_assert!(possible_states.len() == concrete_states.len());

        if possible_states.none() {
            return None;
        };
        if possible_states.iter().filter(|p| *p).count() == 1 {
            return Some(0.);
        };

        // Counts the number of possible states permitted by the UncertainCell
        let possible_state_count: usize = concrete_states.iter()
            .map(|&(_, count)| count)
            .zip(possible_states.iter())
            .filter(|&(_, p)| p)
            .map(|(count, _)| count)
            .sum();

        let possible_state_count = possible_state_count as f64;
        let entropy: f64 = concrete_states.iter()
            .map(|&(_, count)| count)
            .zip(possible_states.iter())
            .filter(|&(_, p)| p)
            .map(|(count, _)| {
                let x = count as f64 / possible_state_count;
                x * x.ln()
            })
            .map(|x| x * x.ln())
            .sum();

        Some(-entropy)

    }

    pub fn collapse<T>(&self, concrete_states: &[(T, usize)]) {
        /// Marks all but a single state of the BitVec as forbidden, randomly chosen
        /// from the states still permitted and weighted by their frequency in the original image.
        let mut possible_states = self.possible_states.borrow_mut();
        let chosen_state = utils::masked_weighted_choice(concrete_states, &*possible_states);
        possible_states.clear();
        possible_states.set(chosen_state, true);
    }
}


struct OverlappingModel {
    model: Array2<UncertainCell>,
    palette: Vec<RGB>,
    states: Vec<(Array2<RGB>, usize)>,
    state_size: usize,
}

impl OverlappingModel {
    pub fn from_seed_image(seed_image: SeedImage,
                           output_dims: (usize, usize),
                           block_size: usize)
                           -> OverlappingModel {
        let palette = OverlappingModel::build_color_palette(&seed_image.image_data);
        let states = OverlappingModel::build_block_frequency_map(&seed_image.image_data,
                                                                 block_size);

        let num_colors = palette.len();
        let num_states = states.len();
        let (x, y) = output_dims;
        let mut model_data = Vec::<UncertainCell>::with_capacity(x * y);

        for _ in 0..(x * y) {
            model_data.push(UncertainCell::new(num_colors, num_states));
        }
        let model = Array::from_shape_vec((y, x), model_data).unwrap();

        OverlappingModel {
            model: model,
            palette: palette,
            states: states,
            state_size: block_size,
        }
    }

    fn find_lowest_nonzero_entropy_coordinates(&self) -> Result<(usize, usize), ModelError> {
        let mut output: Option<(usize, usize)> = None;
        let mut entropy: f64 = f64::MAX;
        for (index, cell) in self.model.indexed_iter() {
            match cell.entropy(&self.states) {
                None => return Err(ModelError::NoValidStates(index)),
                Some(u) if u > 0. => {
                    if u <= entropy {
                        entropy = u;
                        output = Some(index);
                    } else if u.is_nan() {
                        return Err(ModelError::UnexpectedNaN(index));
                    };
                }
                Some(_) => continue,

            }
        }
        match output {
            None => Err(ModelError::AllStatesDecided),
            Some(u) => Ok(u),
        }
    }

    fn color_to_index(&self, color: &RGB) -> usize {
        self.palette.binary_search(color).expect("Color not found in palette!")
    }

    fn valid_coordinate(&self, coord: (usize, usize)) -> bool {
        let (y, x) = coord;
        let (self_y, self_x) = self.model.dim();
        (y < self_y) && (x < self_x)
    }

    fn valid_states_at_position(&self, position: (usize, usize)) -> Vec<usize> {
        let p = position;
        let mut valid_state_indices = Vec::<usize>::with_capacity(self.states.len());

        'state: for (state_index, state) in self.states.iter().map(|&(ref s, _)| s).enumerate() {
            for (coord, color) in state.indexed_iter() {
                let color = self.color_to_index(color);
                let offset_coord = (p.0 + coord.0, p.1 + coord.1);
                if !self.valid_coordinate(offset_coord) {continue 'state;}
                if !self.model[offset_coord].valid_color(color) {continue 'state;}
            }
            valid_state_indices.push(state_index);


        }

        valid_state_indices
    }

    fn build_color_palette(image_data: &Array2<RGB>) -> Vec<RGB> {
        let mut palette: Vec<RGB> = image_data.iter().cloned().collect();
        palette.sort();
        palette.dedup();
        palette
    }

    fn build_block_frequency_map(image_data: &Array2<RGB>,
                                 block_size: usize)
                                 -> Vec<(Array2<RGB>, usize)> {
        let mut block_counts = HashMap::new();

        //TODO augment with rotations and reflections

        for block in image_data.windows((block_size, block_size)) {
            let block = block.to_owned();
            let count = block_counts.entry(block).or_insert(0);
            *count += 1;
        }

        block_counts.into_iter().collect()
    }
}


enum ModelError {
    NoValidStates((usize, usize)),
    UnexpectedNaN((usize, usize)),
    AllStatesDecided,
}
