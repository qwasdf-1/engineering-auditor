use super::*;

pub(super) fn find_clone_classes(files: &[TokenizedFile]) -> Vec<Finding> {
    let seeds = seed_windows(files);
    let mut classes: BTreeMap<(u64, usize), CloneClass> = BTreeMap::new();
    for locations in seeds.values().filter(|locations| locations.len() > 1) {
        build_seed_classes(files, locations, &mut classes);
    }
    let mut classes = classes.into_values().collect::<Vec<_>>();
    classes.sort_by(|left, right| {
        right
            .token_count
            .cmp(&left.token_count)
            .then_with(|| canonical_occurrence(left).cmp(&canonical_occurrence(right)))
            .then_with(|| left.token_hash.cmp(&right.token_hash))
    });
    let classes = suppress_overlapping_classes(classes, files.len());
    classes
        .into_iter()
        .filter_map(|class| clone_finding(files, class))
        .collect()
}

fn seed_windows(files: &[TokenizedFile]) -> BTreeMap<u64, Vec<WindowLocation>> {
    let mut seeds: BTreeMap<u64, Vec<WindowLocation>> = BTreeMap::new();
    let power = base_pow(MIN_TOKENS - 1);
    for (file, tokenized) in files.iter().enumerate() {
        let tokens = &tokenized.tokens;
        if tokens.len() < MIN_TOKENS {
            continue;
        }
        let mut hash = hash_window(tokens[..MIN_TOKENS].iter().map(|token| token.hash));
        for start in 0..=tokens.len() - MIN_TOKENS {
            if start > 0 {
                let incoming = tokens[start + MIN_TOKENS - 1].hash;
                hash = roll(hash, tokens[start - 1].hash, incoming, power);
            }
            if window_is_function_bounded(tokens, start) {
                seeds
                    .entry(hash)
                    .or_default()
                    .push(WindowLocation { file, start });
            }
        }
    }
    for locations in seeds.values_mut() {
        locations.sort_unstable();
        locations.dedup();
    }
    seeds
}

fn window_is_function_bounded(tokens: &[DetectionToken], start: usize) -> bool {
    let owner = tokens[start].function_owner;
    owner.is_some() && tokens[start + MIN_TOKENS - 1].function_owner == owner
}

fn build_seed_classes(
    files: &[TokenizedFile],
    locations: &[WindowLocation],
    classes: &mut BTreeMap<(u64, usize), CloneClass>,
) {
    let representative = locations[0];
    for candidate in locations.iter().copied().skip(1) {
        if windows_overlap(representative, candidate, MIN_TOKENS)
            || files[representative.file].package != files[candidate.file].package
            || alpha_map_for(files, representative, candidate, MIN_TOKENS).is_none()
            || has_equal_predecessor(files, representative, candidate)
        {
            continue;
        }
        let token_count = extend_right(files, representative, candidate);
        let first = CloneOccurrence {
            file: representative.file,
            start: representative.start,
            end: representative.start + token_count,
        };
        let second = CloneOccurrence {
            file: candidate.file,
            start: candidate.start,
            end: candidate.start + token_count,
        };
        if occurrence_line_count(files, first) < MIN_LINES
            || occurrence_line_count(files, second) < MIN_LINES
            || !has_sufficient_semantic_reuse(&files[first.file].tokens[first.start..first.end])
        {
            continue;
        }
        let token_hash = parameterized_hash(&files[first.file].tokens[first.start..first.end]);
        let parameters = differing_bindings(files, first, second);
        let class = classes
            .entry((token_hash, token_count))
            .or_insert_with(|| CloneClass {
                token_hash,
                token_count,
                parameters,
                occurrences: BTreeSet::new(),
            });
        class.parameters = class.parameters.max(parameters);
        class.occurrences.insert(first);
        class.occurrences.insert(second);
    }
}

fn has_sufficient_semantic_reuse(tokens: &[DetectionToken]) -> bool {
    const MIN_REUSED_IDENTITIES: usize = 2;
    let mut counts: BTreeMap<(u8, u64), usize> = BTreeMap::new();
    for token in tokens.iter().filter(|token| token.semantic_anchor) {
        *counts
            .entry((token.kind as u8, token.identity))
            .or_default() += 1;
    }
    counts
        .values()
        .filter(|occurrences| **occurrences > 1)
        .take(MIN_REUSED_IDENTITIES)
        .count()
        >= MIN_REUSED_IDENTITIES
}

fn has_equal_predecessor(
    files: &[TokenizedFile],
    left: WindowLocation,
    right: WindowLocation,
) -> bool {
    // The predecessor counts only if its own window could seed a clone: a
    // keyword outside the function (`export`, a decorator) would otherwise
    // suppress every clone that starts at the function itself.
    if left.start == 0
        || right.start == 0
        || !window_is_function_bounded(&files[left.file].tokens, left.start - 1)
        || !window_is_function_bounded(&files[right.file].tokens, right.start - 1)
    {
        return false;
    }
    let Some(mut mapping) = alpha_map_for(files, left, right, MIN_TOKENS) else {
        return false;
    };
    tokens_match(
        files[left.file].tokens[left.start - 1],
        files[right.file].tokens[right.start - 1],
        &mut mapping,
    )
}

fn extend_right(files: &[TokenizedFile], left: WindowLocation, right: WindowLocation) -> usize {
    let left_tokens = &files[left.file].tokens;
    let right_tokens = &files[right.file].tokens;
    let Some(mut mapping) = alpha_map_for(files, left, right, MIN_TOKENS) else {
        return 0;
    };
    let left_owner = left_tokens[left.start].function_owner;
    let right_owner = right_tokens[right.start].function_owner;
    let mut length = MIN_TOKENS;
    while left.start + length < left_tokens.len()
        && right.start + length < right_tokens.len()
        && left_tokens[left.start + length].function_owner == left_owner
        && right_tokens[right.start + length].function_owner == right_owner
        && tokens_match(
            left_tokens[left.start + length],
            right_tokens[right.start + length],
            &mut mapping,
        )
    {
        length += 1;
    }
    length
}

#[derive(Default)]
struct AlphaMap {
    left_to_right: BTreeMap<u64, u64>,
    right_to_left: BTreeMap<u64, u64>,
}

fn alpha_map_for(
    files: &[TokenizedFile],
    left: WindowLocation,
    right: WindowLocation,
    length: usize,
) -> Option<AlphaMap> {
    let mut mapping = AlphaMap::default();
    for offset in 0..length {
        if !tokens_match(
            files[left.file].tokens[left.start + offset],
            files[right.file].tokens[right.start + offset],
            &mut mapping,
        ) {
            return None;
        }
    }
    Some(mapping)
}

fn tokens_match(left: DetectionToken, right: DetectionToken, mapping: &mut AlphaMap) -> bool {
    if left.hash != right.hash || left.kind != right.kind {
        return false;
    }
    if left.kind == TokenKind::Syntax {
        return true;
    }
    let (left_identity, right_identity) = (keyed_identity(left), keyed_identity(right));
    if mapping
        .left_to_right
        .get(&left_identity)
        .is_some_and(|mapped| *mapped != right_identity)
        || mapping
            .right_to_left
            .get(&right_identity)
            .is_some_and(|mapped| *mapped != left_identity)
    {
        return false;
    }
    mapping.left_to_right.insert(left_identity, right_identity);
    mapping.right_to_left.insert(right_identity, left_identity);
    true
}

/// A token's identity told apart by kind, so a name and an equal literal
/// never map to each other.
fn keyed_identity(token: DetectionToken) -> u64 {
    token.identity ^ (token.kind as u64).wrapping_mul(HASH_BASE)
}

/// Distinct identifier and literal bindings that differ between two
/// occurrences: the parameters a shared extraction would have to take. A name
/// declared inside the clone (`let x =`, `x = ...`, `for x in`) is a local of
/// the extraction, not a parameter; an attribute write (`self.x = x`) is not
/// a declaration.
fn differing_bindings(
    files: &[TokenizedFile],
    left: CloneOccurrence,
    right: CloneOccurrence,
) -> usize {
    let left_tokens = &files[left.file].tokens[left.start..left.end];
    let right_tokens = &files[right.file].tokens[right.start..right.end];
    let mut seen = BTreeSet::new();
    let mut parameters = BTreeSet::new();
    for (index, (left, right)) in left_tokens.iter().zip(right_tokens).enumerate() {
        if left.kind == TokenKind::Syntax || !seen.insert(left.identity) {
            continue;
        }
        if left.identity != right.identity && !is_declaration(left_tokens, index) {
            parameters.insert((left.identity, right.identity));
        }
    }
    parameters.len()
}

fn is_declaration(tokens: &[DetectionToken], index: usize) -> bool {
    let is = |token: Option<&DetectionToken>, texts: &[&str]| {
        token.is_some_and(|token| {
            token.kind == TokenKind::Syntax
                && texts.iter().any(|text| token.identity == fnv1a_hash(text))
        })
    };
    let previous = index
        .checked_sub(1)
        .and_then(|previous| tokens.get(previous));
    is(
        previous,
        &[
            "let", "var", "const", "mut", "for", "fn", "def", "function", "func",
        ],
    ) || (is(tokens.get(index + 1), &["=", ":="]) && !is(previous, &[".", "->", "::"]))
}

fn parameterized_hash(tokens: &[DetectionToken]) -> u64 {
    let mut identifiers = BTreeMap::new();
    let mut next_identifier = 1u64;
    hash_window(tokens.iter().map(|token| {
        if token.kind == TokenKind::Syntax {
            return token.hash;
        }
        let ordinal = *identifiers
            .entry(keyed_identity(*token))
            .or_insert_with(|| {
                let ordinal = next_identifier;
                next_identifier += 1;
                ordinal
            });
        token.hash ^ ordinal.wrapping_mul(FNV1A_PRIME)
    }))
}

fn windows_overlap(left: WindowLocation, right: WindowLocation, length: usize) -> bool {
    left.file == right.file
        && left.start < right.start + length
        && right.start < left.start + length
}

fn suppress_overlapping_classes(classes: Vec<CloneClass>, file_count: usize) -> Vec<CloneClass> {
    let mut occupied = vec![Vec::<(usize, usize)>::new(); file_count];
    let mut accepted = Vec::new();
    for mut class in classes {
        let mut occurrences = class.occurrences.into_iter().collect::<Vec<_>>();
        occurrences.sort_by_key(|occurrence| (occurrence.file, occurrence.start, occurrence.end));
        let mut retained = Vec::new();
        for occurrence in occurrences {
            let range = (occurrence.start, occurrence.end);
            let overlaps = |other: (usize, usize)| ranges_overlap(other, range);
            if occupied[occurrence.file].iter().copied().any(overlaps)
                || retained
                    .iter()
                    .filter(|other: &&CloneOccurrence| other.file == occurrence.file)
                    .any(|other| overlaps((other.start, other.end)))
            {
                continue;
            }
            retained.push(occurrence);
        }
        if retained.len() < 2 {
            continue;
        }
        for occurrence in &retained {
            occupied[occurrence.file].push((occurrence.start, occurrence.end));
        }
        class.occurrences = retained.into_iter().collect();
        accepted.push(class);
    }
    accepted
}

fn ranges_overlap(left: (usize, usize), right: (usize, usize)) -> bool {
    left.0 < right.1 && right.0 < left.1
}

pub(super) fn fnv1a_hash(value: &str) -> u64 {
    value.bytes().fold(FNV1A_OFFSET_BASIS, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(FNV1A_PRIME)
    })
}

pub(super) fn token_hash(kind: TokenKind, value: &str) -> u64 {
    fnv1a_hash(value) ^ kind as u64
}

pub(super) fn hash_window(hashes: impl IntoIterator<Item = u64>) -> u64 {
    hashes.into_iter().fold(0, |hash, token| {
        hash.wrapping_mul(HASH_BASE).wrapping_add(token)
    })
}

pub(super) fn roll(current: u64, outgoing: u64, incoming: u64, power: u64) -> u64 {
    current
        .wrapping_sub(outgoing.wrapping_mul(power))
        .wrapping_mul(HASH_BASE)
        .wrapping_add(incoming)
}

pub(super) fn base_pow(exponent: usize) -> u64 {
    (0..exponent).fold(1u64, |power, _| power.wrapping_mul(HASH_BASE))
}
