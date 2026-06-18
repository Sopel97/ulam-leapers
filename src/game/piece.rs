use crate::game::persist::uls::{UlsAttackVector, ULS_MAX_ATTACK_VECTOR_COORD};
use crate::math::coords::{symmetries, GridPoint, GridVector};
use std::cmp;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io::{ErrorKind, Read, Write};

static LEAPER_NAMES: std::sync::LazyLock<BTreeMap<(i32, i32), &str>> =
    std::sync::LazyLock::new(|| {
        let mut names = BTreeMap::<(i32, i32), &str>::new();
        names.insert((0, 1), "Vazir");
        names.insert((0, 2), "Dabbaba");
        names.insert((0, 3), "Threeleaper");
        names.insert((0, 4), "Fourleaper");
        names.insert((1, 1), "Ferz");
        names.insert((1, 2), "Knight");
        names.insert((1, 3), "Camel");
        names.insert((1, 4), "Giraffe");
        names.insert((2, 2), "Alfil");
        names.insert((2, 3), "Zebra");
        names.insert((2, 4), "Stag");
        names.insert((3, 3), "Tripper");
        names.insert((3, 4), "Antelope");
        names.insert((4, 4), "Commuter");
        names
    });

pub fn leaper_name_from_attack_vector(v: &GridVector) -> Option<&'static str> {
    let canonical = GridVector::new(
        cmp::min(v.x.abs(), v.y.abs()),
        cmp::max(v.x.abs(), v.y.abs()),
    );

    LEAPER_NAMES.get(&(canonical.x, canonical.y)).copied()
}

/// The stored attack vectors are fully ordered, so equality comparison is well-defined.
#[derive(Debug, Eq, PartialEq, Clone)]
pub struct LeaperAttacks {
    attack_vectors: Vec<GridVector>,
}

impl LeaperAttacks {
    pub fn from_offsets(offsets: HashSet<GridVector>) -> Self {
        let mut sorted_offsets: Vec<_> = offsets.into_iter().collect();
        sorted_offsets.sort();
        Self {
            attack_vectors: sorted_offsets,
        }
    }

    pub fn from_canonical(v: &GridVector) -> LeaperAttacks {
        LeaperAttacks::from_offsets(symmetries(v).collect())
    }

    pub fn from_canonicals<'a>(vs: impl Iterator<Item = &'a GridVector>) -> LeaperAttacks {
        LeaperAttacks::from_offsets(vs.flat_map(symmetries).collect::<HashSet<GridVector>>())
    }

    pub fn get_attacks_from(&self, base: &GridPoint) -> impl Iterator<Item = GridPoint> {
        self.attack_vectors.iter().map(move |&v| *base + v)
    }

    pub fn attack_vectors(&self) -> &[GridVector] {
        self.attack_vectors.as_slice()
    }

    pub fn radius(&self) -> usize {
        self.attack_vectors
            .iter()
            .flat_map(|v| [v.x.unsigned_abs(), v.y.unsigned_abs()].into_iter())
            .max()
            .unwrap_or(0) as usize
    }

    pub fn is_symmetric(&self) -> bool {
        // NOTE: quite a naive implementation, but should be fine.
        let all_symmetric = Self::from_canonicals(self.attack_vectors.iter());
        let self_set = self.attack_vectors.iter().collect::<BTreeSet<_>>();
        let all_symmetric_set = all_symmetric.attack_vectors.iter().collect::<BTreeSet<_>>();
        self_set == all_symmetric_set
    }
}

impl LeaperAttacks {
    pub fn from_uls(uls_attack_vectors: Vec<UlsAttackVector>) -> Self {
        let attack_vectors = uls_attack_vectors
            .into_iter()
            .map(|v| GridVector::new(v.x as i32, v.y as i32))
            .collect();

        Self { attack_vectors }
    }
}

fn err_on_attack_offset_too_large(attack_vectors: &[GridVector]) -> std::io::Result<()> {
    for attack_vector in attack_vectors.iter() {
        if attack_vector.x.unsigned_abs() as usize > ULS_MAX_ATTACK_VECTOR_COORD as usize
            || attack_vector.y.unsigned_abs() as usize > ULS_MAX_ATTACK_VECTOR_COORD as usize
        {
            return Err(std::io::Error::new(
                ErrorKind::InvalidData,
                format!("Attack offset larger than {}", ULS_MAX_ATTACK_VECTOR_COORD),
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
