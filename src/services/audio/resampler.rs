pub fn resample_24k_to_16k(input: &[i16]) -> Vec<i16> {
    let input_len = input.len();
    // Ratio is 16/24 = 2/3.
    let output_len = (input_len * 2) / 3;
    let mut output = Vec::with_capacity(output_len);

    for i in 0..output_len {
        // Calculate position in input
        // out_index * 24 / 16 = out_index * 1.5
        let pos = i as f32 * 1.5;
        let index = pos.floor() as usize;
        let frac = pos - index as f32;

        if index + 1 < input_len {
            let s0 = input[index] as f32;
            let s1 = input[index + 1] as f32;
            let val = s0 + (s1 - s0) * frac;
            output.push(val as i16);
        } else if index < input_len {
            output.push(input[index]);
        }
    }
    output
}
