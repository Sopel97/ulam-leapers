use crate::io::{ReadFrom, WriteTo};
use crate::math::coords::{GridPoint, GridVector};
use std::collections::{BTreeSet, HashSet};
use std::io::{ErrorKind, Read, Write};

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct LeaperAttacks {
    attack_vectors: Vec<GridVector>,
}

fn symmetries(v: &GridVector) -> impl Iterator<Item = GridVector> {
    // We could have assembled these via different cases instead of always computing all
    // of them and then deduplicating, but this is simpler and performance does not matter here.
    [
        GridVector::new(v.x, v.y),
        GridVector::new(-v.y, v.x),
        GridVector::new(-v.x, -v.y),
        GridVector::new(v.y, -v.x),
        GridVector::new(-v.x, v.y),
        GridVector::new(v.y, v.x),
        GridVector::new(v.x, -v.y),
        GridVector::new(-v.y, -v.x),
    ]
    .into_iter()
    .collect::<HashSet<GridVector>>()
    .into_iter()
}

impl LeaperAttacks {
    pub fn from_offsets(offsets: HashSet<GridVector>) -> Self {
        Self {
            attack_vectors: offsets.into_iter().collect(),
        }
    }

    pub fn from_canonical(v: &GridVector) -> LeaperAttacks {
        LeaperAttacks {
            attack_vectors: symmetries(v).collect(),
        }
    }

    pub fn from_canonicals<'a>(vs: impl Iterator<Item = &'a GridVector>) -> LeaperAttacks {
        LeaperAttacks {
            attack_vectors: vs
                .flat_map(symmetries)
                .collect::<HashSet<GridVector>>() // collect to a set first to deduplicate
                .into_iter()
                .collect(), // recollect into a vec
        }
    }

    pub fn get_attacks_from(&self, base: &GridPoint) -> impl Iterator<Item = GridPoint> {
        self.attack_vectors.iter().map(move |&v| *base + v)
    }
}

pub const ULS_MAX_ATTACK_OFFSET: usize = 127;

fn err_on_attack_offset_too_large(attack_vectors: &[GridVector]) -> std::io::Result<()> {
    for attack_vector in attack_vectors.iter() {
        if attack_vector.x.unsigned_abs() as usize > ULS_MAX_ATTACK_OFFSET
            || attack_vector.y.unsigned_abs() as usize > ULS_MAX_ATTACK_OFFSET
        {
            return Err(std::io::Error::new(
                ErrorKind::InvalidData,
                format!("Attack offset larger than {}", ULS_MAX_ATTACK_OFFSET),
            ));
        }
    }
    Ok(())
}

fn err_on_duplicate_attack_vectors(attack_vectors: &[GridVector]) -> std::io::Result<()> {
    if attack_vectors.iter().collect::<BTreeSet<_>>().len() != attack_vectors.len() {
        Err(std::io::Error::new(
            ErrorKind::InvalidData,
            "Duplicated attack offsets found.",
        ))
    } else {
        Ok(())
    }
}

impl WriteTo for LeaperAttacks {
    fn write_to(&self, writer: &mut impl Write) -> std::io::Result<()> {
        err_on_attack_offset_too_large(&self.attack_vectors)?;
        self.attack_vectors.write_to(writer)
    }
}

impl ReadFrom for LeaperAttacks {
    fn read_from(reader: &mut impl Read) -> std::io::Result<Self> {
        let attack_vectors = Vec::<GridVector>::read_from(reader)?;
        err_on_attack_offset_too_large(&attack_vectors)?;
        err_on_duplicate_attack_vectors(&attack_vectors)?;

        Ok(LeaperAttacks { attack_vectors })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symmetries_generates_8_for_asymmetric_vector() {
        let v = GridVector::new(1, 2);

        let result: Vec<_> = symmetries(&v).collect();

        assert!(result.contains(&GridVector::new(1, 2)));
        assert!(result.contains(&GridVector::new(1, -2)));
        assert!(result.contains(&GridVector::new(-1, 2)));
        assert!(result.contains(&GridVector::new(-1, -2)));
        assert!(result.contains(&GridVector::new(2, 1)));
        assert!(result.contains(&GridVector::new(2, -1)));
        assert!(result.contains(&GridVector::new(-2, 1)));
        assert!(result.contains(&GridVector::new(-2, -1)));

        assert_eq!(result.len(), 8);
    }

    #[test]
    fn symmetries_generates_4_for_orthogonal_vector() {
        let v = GridVector::new(0, 2);

        let result: Vec<_> = symmetries(&v).collect();

        assert!(result.contains(&GridVector::new(0, 2)));
        assert!(result.contains(&GridVector::new(0, -2)));
        assert!(result.contains(&GridVector::new(2, 0)));
        assert!(result.contains(&GridVector::new(-2, 0)));

        assert_eq!(result.len(), 4);
    }

    #[test]
    fn symmetries_generates_4_for_diagonal_vector() {
        let v = GridVector::new(2, 2);

        let result: Vec<_> = symmetries(&v).collect();

        assert!(result.contains(&GridVector::new(2, 2)));
        assert!(result.contains(&GridVector::new(2, -2)));
        assert!(result.contains(&GridVector::new(-2, 2)));
        assert!(result.contains(&GridVector::new(-2, -2)));

        assert_eq!(result.len(), 4);
    }

    #[test]
    fn from_canonical_collects_all_symmetries() {
        let attacks = LeaperAttacks::from_canonical(&GridVector::new(1, 2));

        assert_eq!(attacks.attack_vectors.len(), 8);
    }

    #[test]
    fn from_canonicals_deduplicates_vectors() {
        let canonical = [
            GridVector::new(1, 2),
            GridVector::new(2, 1), // same symmetry class
        ];

        let attacks = LeaperAttacks::from_canonicals(canonical.iter());

        assert_eq!(attacks.attack_vectors.len(), 8);
    }

    #[test]
    fn get_attacks_from_offsets_base_position() {
        let attacks = LeaperAttacks::from_canonical(&GridVector::new(1, 2));

        let base = GridPoint::new(10, 20);

        let result: Vec<_> = attacks.get_attacks_from(&base).collect();

        assert!(result.contains(&GridPoint::new(11, 22)));
        assert!(result.contains(&GridPoint::new(11, 18)));
        assert!(result.contains(&GridPoint::new(9, 22)));
        assert!(result.contains(&GridPoint::new(9, 18)));
        assert!(result.contains(&GridPoint::new(12, 21)));
        assert!(result.contains(&GridPoint::new(12, 19)));
        assert!(result.contains(&GridPoint::new(8, 21)));
        assert!(result.contains(&GridPoint::new(8, 19)));

        assert_eq!(result.len(), 8);
    }
}
