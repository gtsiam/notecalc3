use crate::calc::{CalcResult, CalcResultType};
use crate::units::units::Units;
use crate::{ResultFormat, ResultLengths};
use byteorder::WriteBytesExt;
use rust_decimal::prelude::*;
use rust_decimal::Decimal;
use smallvec::SmallVec;
use std::io::Cursor;

pub fn render_result(
    units: &Units,
    result: &CalcResult,
    format: &ResultFormat,
    there_was_unit_conversion: bool,
    decimal_count: Option<usize>,
    use_grouping: bool,
) -> String {
    let mut c = Cursor::new(Vec::with_capacity(64));
    render_result_into(
        units,
        result,
        format,
        there_was_unit_conversion,
        &mut c,
        decimal_count,
        use_grouping,
    );
    return unsafe { String::from_utf8_unchecked(c.into_inner()) };
}

pub fn render_result_into(
    units: &Units,
    result: &CalcResult,
    format: &ResultFormat,
    there_was_unit_conversion: bool,
    f: &mut impl std::io::Write,
    decimal_count: Option<usize>,
    use_grouping: bool,
) -> ResultLengths {
    match &result.typ {
        CalcResultType::Quantity(num, unit) => {
            let final_unit = if there_was_unit_conversion {
                None
            } else {
                unit.simplify(units)
            };
            let unit = final_unit.as_ref().unwrap_or(unit);
            if unit.units.is_empty() {
                num_to_string(f, &num, &ResultFormat::Dec, decimal_count, use_grouping)
            } else {
                let denormalized_num = unit.from_base_to_this_unit(num);
                if let Some(denormalized_num) = denormalized_num {
                    let mut lens = num_to_string(
                        f,
                        &denormalized_num,
                        &ResultFormat::Dec,
                        decimal_count,
                        use_grouping,
                    );
                    f.write_u8(b' ').expect("");
                    // TODO:mem to_string -> into(buf)
                    // implement a into(std::io:Write) method for UnitOutput
                    for ch in unit.to_string().as_bytes() {
                        f.write_u8(*ch).expect("");
                        lens.unit_part_len += 1;
                    }
                    lens
                } else {
                    ResultLengths {
                        int_part_len: 0,
                        frac_part_len: 0,
                        unit_part_len: 0,
                    }
                }
            }
        }
        CalcResultType::Unit(unit) => {
            // TODO:mem to_string -> into(buf)
            // implement a into(std::io:Write) method for UnitOutput
            let mut len = 0;
            for ch in unit.to_string().as_bytes() {
                f.write_u8(*ch).expect("");
                len += 1;
            }
            ResultLengths {
                int_part_len: 0,
                frac_part_len: 0,
                unit_part_len: len,
            }
        }
        CalcResultType::Number(num) => {
            // TODO optimize
            num_to_string(f, num, format, decimal_count, use_grouping)
        }
        CalcResultType::Percentage(num) => {
            let mut lens = num_to_string(f, num, &ResultFormat::Dec, decimal_count, use_grouping);
            f.write_u8(b' ').expect("");
            f.write_u8(b'%').expect("");
            lens.unit_part_len += 1;
            lens
        }
        CalcResultType::Matrix(mat) => {
            f.write_u8(b'[').expect("");
            for row_i in 0..mat.row_count {
                if row_i > 0 {
                    f.write_u8(b';').expect("");
                    f.write_u8(b' ').expect("");
                }
                for col_i in 0..mat.col_count {
                    if col_i > 0 {
                        f.write_u8(b',').expect("");
                        f.write_u8(b' ').expect("");
                    }
                    let cell = &mat.cells[row_i * mat.col_count + col_i];
                    render_result_into(units, cell, format, false, f, decimal_count, use_grouping);
                }
            }
            f.write_u8(b']').expect("");
            ResultLengths {
                int_part_len: 0,
                frac_part_len: 0,
                unit_part_len: 0,
            }
        }
    }
}

fn num_to_string(
    f: &mut impl std::io::Write,
    num: &Decimal,
    format: &ResultFormat,
    decimal_count: Option<usize>,
    use_grouping: bool,
) -> ResultLengths {
    let num_a = if *format != ResultFormat::Dec && num.trunc() == *num {
        Some(num.clone())
    } else if let Some(decimal_count) = decimal_count {
        let mut result = num.clone();
        result.rescale(decimal_count as u32);
        Some(result.normalize())
    } else {
        let with_scale_0 = num.trunc();
        if *num == with_scale_0 {
            Some(with_scale_0)
        } else {
            None
        }
    };
    let num = num_a.as_ref().unwrap_or(num);

    return if *format == ResultFormat::Bin || *format == ResultFormat::Hex {
        if let Some(n) = num.to_i64() {
            let ss = if *format == ResultFormat::Bin {
                format!("{:b}", n)
            } else {
                format!("{:X}", n)
            };
            ResultLengths {
                int_part_len: apply_grouping(
                    f,
                    &ss,
                    if use_grouping {
                        if *format == ResultFormat::Bin {
                            8
                        } else {
                            2
                        }
                    } else {
                        std::i32::MAX as usize
                    },
                ),
                frac_part_len: 0,
                unit_part_len: 0,
            }
        } else {
            f.write_u8(b'E').expect("");
            f.write_u8(b'r').expect("");
            f.write_u8(b'r').expect("");
            ResultLengths {
                int_part_len: 3,
                frac_part_len: 0,
                unit_part_len: 0,
            }
        }
    } else {
        // TODO to_string opt
        let string = num.to_string();
        if let Some(pos) = string.bytes().position(|it| it == b'.') {
            let (int_part, fract_part) = string.split_at(pos);
            let int_len = apply_grouping(
                f,
                &int_part,
                if use_grouping {
                    3
                } else {
                    std::i32::MAX as usize
                },
            );
            for ch in fract_part.as_bytes() {
                f.write_u8(*ch).expect("");
            }
            ResultLengths {
                int_part_len: int_len,
                frac_part_len: fract_part.len(),
                unit_part_len: 0,
            }
        } else {
            ResultLengths {
                int_part_len: apply_grouping(
                    f,
                    &string,
                    if use_grouping {
                        3
                    } else {
                        std::i32::MAX as usize
                    },
                ),
                frac_part_len: 0,
                unit_part_len: 0,
            }
        }
    };
}

fn apply_grouping(f: &mut impl std::io::Write, ss: &str, group_size: usize) -> usize {
    // TODO isnt it too much/is it enough?
    let mut buf: SmallVec<[u8; 128]> = SmallVec::with_capacity(ss.len());
    for ch in ss.as_bytes() {
        buf.push(*ch);
    }
    let buff = &mut buf[0..ss.len()];
    buff.reverse();
    let mut len = 0;
    for (i, group) in buff.chunks(group_size).rev().enumerate() {
        if i > 0 {
            f.write_u8(b' ').expect("");
            len += 1;
        }
        for ch in group.iter().rev() {
            f.write_u8(*ch).expect("");
            len += 1;
        }
    }
    return len;
}

pub fn get_int_frac_part_len(cell_str: &str) -> ResultLengths {
    let mut int_part_len = 0;
    let mut frac_part_len = 0;
    let mut unit_part_len = 0;
    let mut was_point = false;
    let mut was_space = false;
    let mut only_digits_or_space_so_far = true;
    for ch in cell_str.as_bytes() {
        if *ch == b'.' {
            was_point = true;
            only_digits_or_space_so_far = false;
        } else if *ch == b' ' {
            was_space = true;
        }
        if was_space {
            if only_digits_or_space_so_far && ch.is_ascii_digit() {
                // this space was just a thousand separator
                int_part_len += 1;
                if unit_part_len > 0 {
                    // 2 000, that space was registered as unit, so add it to int_part
                    int_part_len += 1;
                }
                unit_part_len = 0;
            } else {
                if only_digits_or_space_so_far && !ch.is_ascii_whitespace() {
                    only_digits_or_space_so_far = false;
                }
                unit_part_len += 1;
            }
        } else if was_point {
            frac_part_len += 1;
        } else {
            int_part_len += 1;
        }
    }
    return ResultLengths {
        int_part_len,
        frac_part_len,
        unit_part_len,
    };
}

// TODO: really hack and ugly and slow
// pub fn strip_trailing_zeroes(num: &BigDecimal) -> BigDecimal {
//     let (_, mut scale) = num.as_bigint_and_exponent();
//     let mut result = num.clone();
//     loop {
//         if scale == 0 {
//             break;
//         }
//         let scaled = result.with_scale(scale - 1);
//         if &scaled == num {
//             result = scaled;
//         } else {
//             break;
//         }
//         scale -= 1;
//     }
//     return result;
// }
