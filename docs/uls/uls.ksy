meta:
  id: uls
  title: Ulam Leapers Simulation v1.0
  file-extension: uls
  endian: le

seq:
  - id: magic
    contents: 'ULS_v1.0'

  - id: turn_count
    type: u8

  - id: num_players
    type: u1

  - id: players
    type: player
    repeat: expr
    repeat-expr: num_players

  - id: chunker
    type: chunker

  - id: num_chunks
    type: u4

  - id: chunks
    type: chunk
    repeat: expr
    repeat-expr: num_chunks

types:

  player:
    seq:
      - id: spiral_position
        type: u8

      - id: enemies_mask
        type: u8

      - id: num_attack_vectors
        type: u1

      - id: attack_vectors
        type: attack_vector
        repeat: expr
        repeat-expr: num_attack_vectors

  attack_vector:
    seq:
      - id: x
        type: s1
      - id: y
        type: s1

  chunker:
    seq:
      - id: strip_length
        type: u2

      - id: strip_thickness
        type: u2

  chunk:
    seq:
      - id: origin_x
        type: s4

      - id: origin_y
        type: s4

      - id: transform
        type: u1
        enum: chunk_transform

      - id: compression
        type: u1
        enum: compression_kind

      - id: len_blob
        type: u4

      - id: blob
        size: len_blob

enums:

  compression_kind:
    0: none
    1: zstd

  chunk_transform:
    0: none
    1: transposition