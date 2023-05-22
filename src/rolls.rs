use crate::errors::WakeBotError;
use fancy_regex::Regex;
use rand::Rng;
use shunting::{MathContext, ShuntingParser};

pub const INDIVIDUAL_ROLL_REGEX: &str = r"(\d+)?d(\d+)((k|(kh)|(kl))(\d+))?";
pub const ROLL_WITH_MODIFIERS_REGEX: &str =
    r"(\d+)?d(\d+)((k|(kh)|(kl))(\d+))?(( ?[+*\/-] ?\d+(?!d))*)";
pub const ROLL_REGEX: &str = r"^(((\d+)?d(\d+)((k|(kh)|(kl))(\d+))?)| |\d+|[+*/)()-])+$";
pub const ROLL_COMMAND_REGEX: &str = r"^!(((\d+)?d(\d+)((k|(kh)|(kl))(\d+))?)| |\d+|[+*/)()-])+$";
const MAX_QUANTITY: usize = 1000;

// Need to create human-readable summary of rolls

fn resolve_dice_roll(
    input: &str,
) -> Result<(String, Vec<i32>, Vec<i32>, String, String), WakeBotError> {
    let mut roll_string = String::from(input);
    let roll_regex = Regex::new(INDIVIDUAL_ROLL_REGEX).unwrap();
    if !roll_regex.is_match(&roll_string).unwrap_or(false) {
        return Err(WakeBotError::new(
            "Invalid argument passed to resolve_dice_roll.",
        ));
    }

    let (capture, range, individual_roll) = if let Ok(cap) = roll_regex.captures(&roll_string) {
        let mat = roll_regex.find(&roll_string).unwrap().unwrap();
        (
            cap.unwrap(),
            mat.start()..=mat.end() - 1,
            String::from(mat.as_str()),
        )
    } else {
        panic!("No match for individual roll regex in resolve_dice_roll.");
    };
    let quantity = if let Some(_) = capture.get(1) {
        (&capture[1]).parse::<usize>().unwrap()
    } else {
        1
    };
    if quantity > MAX_QUANTITY {
        panic!("Max number of dice is {}", MAX_QUANTITY);
    }
    let max = (&capture[2]).parse::<i32>().unwrap();
    let mut dice_result = 0;
    let mut rolls = vec![];
    let mut discarded_rolls = vec![];
    for _ in 0..quantity {
        let roll_result: i32 = rand::thread_rng().gen_range(1..=max);
        rolls.push(roll_result);
    }
    let advantage_type = if let Some(_) = capture.get(4) {
        Some(&capture[4])
    } else {
        None
    };
    if let Some(t) = advantage_type {
        rolls.sort();
        let count = (&capture[7]).parse::<usize>().unwrap();
        if count < quantity {
            if t.eq("k") || t.eq("kh") {
                rolls.reverse();
            }
            discarded_rolls = rolls.splice(count.., vec![]).collect::<Vec<i32>>();
        }
    }
    println!(
        "Rolled {}, {} = {}",
        roll_string,
        rolls
            .iter()
            .map(|n| {
                dice_result += n;
                n.to_string()
            })
            .collect::<Vec<String>>()
            .join(" + "),
        dice_result
    );
    let roll_regex_with_modifiers = Regex::new(ROLL_WITH_MODIFIERS_REGEX).unwrap();
    let modifier_capture = roll_regex_with_modifiers.captures(input);
    let modifier_string = if let Ok(Some(mod_cap)) = modifier_capture {
        if let Some(_) = mod_cap.get(8) {
            let s = String::from(&mod_cap[8]);
            s.replace(" ", "").chars().fold(String::new(), |acc, char| {
                if char == '-' || char == '+' || char == '*' || char == '/' || char == '=' {
                    acc + &format!(" {} ", char)
                } else {
                    acc + &String::from(char)
                }
            })
        } else {
            String::from("")
        }
    } else {
        String::from("")
    };
    roll_string.replace_range(range, &dice_result.to_string());
    let expr = ShuntingParser::parse_str(&roll_string).unwrap();
    let result = MathContext::new().eval(&expr).unwrap();
    let result = result.round() as i64;
    Ok((
        result.to_string(),
        rolls,
        discarded_rolls,
        modifier_string,
        individual_roll,
    ))
}

#[derive(std::fmt::Debug)]
pub struct RollResult {
    total: String,
    roll_string: String,
    applied_rolls: Vec<i32>,
    discarded_rolls: Vec<i32>,
    modifier_string: String,
    individual_roll: String,
}

pub fn calculate_roll_string(roll: &str) -> (f64, Vec<RollResult>) {
    let regex = Regex::new(ROLL_WITH_MODIFIERS_REGEX).unwrap();
    let mut roll = String::from(roll);

    let mut done = false;
    let mut roll_representation: Vec<RollResult> = vec![];
    while !done {
        let mut resolved_roll = None;
        let range = regex.find(&roll).map(|mat| {
            if mat.is_none() {
                return None;
            }
            let mat = mat.unwrap();
            if let Ok((result, rolls, discarded_rolls, modifier_string, individual_roll)) =
                resolve_dice_roll(mat.as_str())
            {
                resolved_roll = Some(result);
                roll_representation.push(RollResult {
                    total: resolved_roll.clone().unwrap(),
                    roll_string: String::from(mat.as_str()),
                    applied_rolls: rolls,
                    discarded_rolls,
                    modifier_string,
                    individual_roll,
                });
            } else {
                panic!("Matched w/ range but no dice resolution.");
            }
            Some(mat.start()..mat.end())
        });
        if let Ok(Some(r)) = range {
            roll.replace_range(r, &resolved_roll.unwrap());
        } else {
            done = true;
        }
    }
    let roll_sans_exclamation = if roll.starts_with("!") {
        &roll[1..]
    } else {
        &roll
    };
    let expr = ShuntingParser::parse_str(roll_sans_exclamation).unwrap();
    let result = MathContext::new().eval(&expr).unwrap();
    (result, roll_representation)
}

pub fn format_rolls_result(original_string: &str, input: (f64, Vec<RollResult>)) -> String {
    let (result, rolls) = input;
    let d20_regex = Regex::new(r"^\d+?d20").unwrap();
    format!(
        "{}\n{}\n{}",
        original_string,
        rolls
            .iter()
            .map(
                |RollResult {
                     total,
                     roll_string,
                     applied_rolls,
                     discarded_rolls,
                     modifier_string,
                     individual_roll,
                 }| {
                    format!(
                        "{} ({}{}{}) {} = {}{}",
                        individual_roll,
                        applied_rolls
                            .iter()
                            .map(|n| n.to_string())
                            .collect::<Vec<String>>()
                            .join(" + "),
                        if discarded_rolls.len() == 0 {
                            String::from("")
                        } else {
                            String::from(", ")
                                + &discarded_rolls
                                    .iter()
                                    .map(|n| String::from("~~") + &n.to_string() + "~~")
                                    .collect::<Vec<String>>()
                                    .join(" + ")
                        },
                        if applied_rolls.len() + discarded_rolls.len() > 1 {
                            format!(
                                " = {}",
                                applied_rolls.iter().fold(0, |mut acc, curr| {
                                    acc += curr;
                                    acc
                                })
                            )
                        } else {
                            String::from("")
                        },
                        modifier_string.trim(),
                        total,
                        {
                            let mut str = String::from("");
                            if d20_regex.is_match(roll_string).unwrap_or(false) {
                                if applied_rolls.contains(&20) {
                                    str += " - **CRITICAL SUCCESS!**";
                                }
                                if applied_rolls.contains(&1) {
                                    str += " - **CRITICAL FAILURE!**";
                                }
                            }
                            str
                        }
                    )
                }
            )
            .collect::<Vec<String>>()
            .join("\n"),
        String::from("**") + &result.to_string() + "**"
    )
}
