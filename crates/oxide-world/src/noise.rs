//! The Java generators the biome tint path needs: `java.util.Random`'s 48-bit
//! LCG, and the simplex generator `NoiseGeneratorPerlin` wraps for the height
//! adjustment.
//!
//! Both are ports of what the decompiled 1.8.9 client runs, not of its text:
//!
//! * `java.util.Random`'s algorithm — the multiplier `0x5DEECE66D`, the addend
//!   `0xB` and the 48-bit mask — is the JDK's, and the client reaches it through
//!   `new Random(seed)` in `BiomeGenBase.java:649-650`. The pinned sequences in
//!   `crates/oxide-world/tests/biome.rs` were printed by the real
//!   `java.util.Random` under the local JVM.
//! * `NoiseGeneratorPerlin.func_151601_a` and the `NoiseGeneratorSimplex`
//!   two-dimensional sample (`func_151605_a`) are the structure vanilla uses;
//!   their constants trace to
//!   `refs/_src/MCP-919/src/minecraft/net/minecraft/world/gen/NoiseGeneratorPerlin.java`
//!   and `.../NoiseGeneratorSimplex.java`, and the values the port answers are
//!   pinned against samples the decompiled classes printed on the JVM.
//!
//! `NoiseGeneratorOctaves` is deliberately not ported: it is built over
//! `NoiseGeneratorImproved`, not over this generator, and the height adjustment
//! never constructs one.

/// The 48-bit linear congruential generator behind `java.util.Random`.
///
/// A caller draws raw bits with [`JavaRandom::next_bits`] or one of the typed
/// draws; the seed is scrambled on construction, exactly as
/// `new java.util.Random(seed)` scrambles it.
#[derive(Debug, Clone)]
pub struct JavaRandom {
    /// The generator's state: the low 48 bits of the seed.
    seed: u64,
}

/// `java.util.Random`'s multiplier.
const MULTIPLIER: u64 = 0x5_DEEC_E66D;
/// `java.util.Random`'s addend.
const ADDEND: u64 = 0xB;
/// The generator's state width: 48 bits.
const STATE_MASK: u64 = (1 << 48) - 1;

impl JavaRandom {
    /// A generator seeded as `new java.util.Random(seed)`: the given seed is
    /// scrambled with the multiplier before the first draw.
    pub fn new(seed: i64) -> Self {
        Self {
            seed: (seed as u64 ^ MULTIPLIER) & STATE_MASK,
        }
    }

    /// The next `bits` bits of the stream: `java.util.Random.next(bits)`.
    ///
    /// The documented draws are 1..=32; the shift count is masked to six bits
    /// and the result is the low 32 bits of the shifted state, which is what the
    /// JVM computes for the same call whatever it is handed.
    pub fn next_bits(&mut self, bits: u32) -> u32 {
        self.seed = self.seed.wrapping_mul(MULTIPLIER).wrapping_add(ADDEND) & STATE_MASK;
        (self.seed >> (48u32.wrapping_sub(bits) & 63)) as u32
    }

    /// The next float in `0.0..1.0`: `java.util.Random.nextFloat`, the top 24
    /// bits of a draw over `2^24`.
    pub fn next_float(&mut self) -> f32 {
        self.next_bits(24) as f32 / (1u32 << 24) as f32
    }

    /// The next double in `0.0..1.0`: `java.util.Random.nextDouble`, a 26-bit
    /// and a 27-bit draw over `2^53`.
    pub fn next_double(&mut self) -> f64 {
        let high = u64::from(self.next_bits(26)) << 27;
        let low = u64::from(self.next_bits(27));
        (high + low) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// A value in `0..bound`: `java.util.Random.nextInt(bound)`.
    ///
    /// A power of two takes the source's scaled-draw shortcut; anything else
    /// takes its rejection loop, which redraws while the drawn value plus the
    /// remaining range would have overflowed a signed 32-bit value. The
    /// source's contract is a positive bound (`java.util.Random.nextInt` throws
    /// otherwise); a bound outside it — or zero, which the algorithm's own
    /// power-of-two branch answers with 0 — cannot be reached from this crate's
    /// call sites, which pass `1..=256` (`NoiseGeneratorSimplex.java:35`).
    pub fn next_int(&mut self, bound: u32) -> u32 {
        if bound & bound.wrapping_neg() == bound {
            return ((u64::from(bound) * u64::from(self.next_bits(31))) >> 31) as u32;
        }
        loop {
            let bits = self.next_bits(31);
            let value = bits % bound;
            let within_contract = bound <= i32::MAX as u32;
            let distorted =
                within_contract && u64::from(bits - value) + u64::from(bound) - 1 > i32::MAX as u64;
            if !distorted {
                return value;
            }
        }
    }
}

/// A sample from the generator structure the height adjustment uses:
/// `NoiseGeneratorPerlin.func_151601_a`.
///
/// One `JavaRandom` seeded with `seed` feeds every octave: each octave draws its
/// own permutation from that stream in turn, is sampled at `x * scale, z * scale`
/// and contributes its value divided by `scale`, and `scale` halves per octave
/// (`NoiseGeneratorPerlin.java:10-33`). The height adjustment builds this with
/// one octave and seed 1234 (`BiomeGenBase.java:649`); zero octaves sum to 0.
///
/// The samples are pure in their arguments, so building and sampling one octave
/// at a time is the same stream as the source's build-them-all-then-sample;
/// sampling draws nothing.
pub fn perlin_sample(seed: i64, octaves: u32, x: f64, z: f64) -> f64 {
    let mut random = JavaRandom::new(seed);
    let mut total = 0.0;
    let mut scale = 1.0;
    for _ in 0..octaves {
        let generator = Simplex::new(&mut random);
        total += generator.sample(x * scale, z * scale) / scale;
        scale /= 2.0;
    }
    total
}

/// One `NoiseGeneratorSimplex`: the shuffled permutation it draws, and the
/// two-dimensional sample `func_151605_a`.
struct Simplex {
    /// 512 entries: the permutation of 0..256, mirrored at +256
    /// (`NoiseGeneratorSimplex.java:23-40`).
    permutation: [u8; 512],
}

/// The twelve gradient vectors (`NoiseGeneratorSimplex.java:7`). The
/// two-dimensional sample reads only each vector's first two components
/// (`func_151604_a`, `:48-51`).
const GRADIENTS: [[i32; 3]; 12] = [
    [1, 1, 0],
    [-1, 1, 0],
    [1, -1, 0],
    [-1, -1, 0],
    [1, 0, 1],
    [-1, 0, 1],
    [1, 0, -1],
    [-1, 0, -1],
    [0, 1, 1],
    [0, -1, 1],
    [0, 1, -1],
    [0, -1, -1],
];

impl Simplex {
    /// A generator drawing from `random` as `new NoiseGeneratorSimplex(random)`.
    fn new(random: &mut JavaRandom) -> Self {
        // Three offsets are drawn before the permutation
        // (`NoiseGeneratorSimplex.java:24-26`). The two-dimensional sample reads
        // none of them — only the array-populating overload does — but the draws
        // advance the stream the permutation below depends on, so they are made
        // and dropped in the source's order.
        let _x_offset = random.next_double() * 256.0;
        let _y_offset = random.next_double() * 256.0;
        let _z_offset = random.next_double() * 256.0;

        let mut permutation = [0u8; 512];
        for (index, entry) in permutation.iter_mut().take(256).enumerate() {
            *entry = index as u8;
        }
        for index in 0..256 {
            let swap = random.next_int((256 - index) as u32) as usize + index;
            permutation.swap(index, swap);
            permutation[index + 256] = permutation[index];
        }
        Self { permutation }
    }

    /// The two-dimensional simplex sample, `func_151605_a`.
    fn sample(&self, x: f64, z: f64) -> f64 {
        // `Math.sqrt(3.0D)` and the two constants derived from it
        // (`NoiseGeneratorSimplex.java:8,13-14`), by the same IEEE operations.
        let sqrt_3 = 3.0f64.sqrt();
        let unskew = 0.5 * (sqrt_3 - 1.0);
        let skew = (3.0 - sqrt_3) / 6.0;

        let offset = (x + z) * unskew;
        let i = floor(x + offset);
        let j = floor(z + offset);
        let corner = f64::from(i + j) * skew;
        let dx0 = x - (f64::from(i) - corner);
        let dz0 = z - (f64::from(j) - corner);
        let (i_step, j_step) = if dx0 > dz0 {
            (1i32, 0i32)
        } else {
            (0i32, 1i32)
        };
        let dx1 = dx0 - f64::from(i_step) + skew;
        let dz1 = dz0 - f64::from(j_step) + skew;
        let dx2 = dx0 - 1.0 + 2.0 * skew;
        let dz2 = dz0 - 1.0 + 2.0 * skew;

        let column = (i & 255) as usize;
        let row = (j & 255) as usize;
        let index_a = column + usize::from(self.permutation[row]);
        let index_b =
            column + i_step as usize + usize::from(self.permutation[row + j_step as usize]);
        let index_c = column + 1 + usize::from(self.permutation[row + 1]);
        let corner_a = self.permutation[index_a] % 12;
        let corner_b = self.permutation[index_b] % 12;
        let corner_c = self.permutation[index_c] % 12;

        70.0 * (contribution(dx0, dz0, corner_a)
            + contribution(dx1, dz1, corner_b)
            + contribution(dx2, dz2, corner_c))
    }
}

/// One corner's falloff-scaled gradient dot (`NoiseGeneratorSimplex.java:88-125`).
fn contribution(dx: f64, dz: f64, gradient: u8) -> f64 {
    let falloff = 0.5 - dx * dx - dz * dz;
    if falloff < 0.0 {
        0.0
    } else {
        let gradient = GRADIENTS[gradient as usize];
        let scaled = falloff * falloff;
        scaled * scaled * (f64::from(gradient[0]) * dx + f64::from(gradient[1]) * dz)
    }
}

/// `func_151607_a` (`NoiseGeneratorSimplex.java:43-46`): the source's own floor —
/// `(int)` for a positive value, one less than the truncated value otherwise.
/// Zero takes the second branch, so it answers -1.
fn floor(value: f64) -> i32 {
    if value > 0.0 {
        value as i32
    } else {
        value as i32 - 1
    }
}
