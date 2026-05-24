// Very simple run length encoding directly into raw bytes.
// Main purpose is to have some simple compressor for testing.
pub fn encode(input: &[u8]) -> Box<[u8]> {
    if input.is_empty() {
        return Box::new([]);
    }

    let mut result = Vec::with_capacity(64);

    let mut iter = input.iter();
    let mut current = *iter.next().unwrap();
    let mut len = 1;
    for v in iter {
        if len != u8::MAX && *v == current {
            len += 1;
        } else {
            result.push(len);
            result.push(current);
            current = *v;
            len = 1;
        }
    }

    result.push(len);
    result.push(current);

    result.into_boxed_slice()
}

pub fn decode(input: &[u8]) -> Box<[u8]> {
    if input.is_empty() {
        return Box::new([]);
    }

    let mut result = Vec::with_capacity(64);
    for chunk in input.chunks_exact(2) {
        let len = chunk[0];
        let val = chunk[1];
        result.extend((0..len).map(|_| val));
    }

    result.into_boxed_slice()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding_empty_array() {
        let input: [u8; 0] = [];
        let encoded = encode(&input);
        assert_eq!(encoded.len(), 0);
    }

    #[test]
    fn test_encode() {
        let input = vec![1u8, 1u8, 2u8, 3u8, 3u8, 3u8, 4u8];
        let encoded = encode(&input);
        assert_eq!(encoded.iter().as_slice(), [2u8, 1u8, 1u8, 2u8, 3u8, 3u8, 1u8, 4u8]);
    }

    #[test]
    fn long_run() {
        let input = vec![0u8; 1024];
        let encoded = encode(&input);
        assert_eq!(encoded.iter().as_slice(), [255u8, 0u8, 255u8, 0u8, 255u8, 0u8, 255u8, 0u8, 4u8, 0u8]);
    }

    #[test]
    fn test_decode() {
        let encoded = [2u8, 1u8, 1u8, 2u8, 3u8, 3u8, 1u8, 4u8];
        let decoded = decode(&encoded);
        assert_eq!(decoded.iter().as_slice(), [1u8, 1u8, 2u8, 3u8, 3u8, 3u8, 4u8]);
    }
}