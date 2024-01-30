use crate::errors::WakeBotError;
use fancy_regex::Regex;
use rand::Rng;
use shunting::{MathContext, ShuntingParser};
use std::fmt::Debug;

// Anything between parens, we are going to attempt to feed into the function. Panic and respond with an error if it isn't properly formatted
// How to handle nested parens? I guess if this is the first step, to "step in" to the paren string, then we'd just step into whatever is in the parens infinitely
const PAREN_REGEX: &str = r"\((.+)\)";

// Isolate the rolls, so we can convert them into numbers and replace them in the string
const ROLL_REGEX: &str =
    r"((\d*)d(\d+)((k|kh|kl)(\d+))?)(( ?[+*/-] ?(\d+(?!\.)|(\d*\.\d+))(?!d))*)";
pub const DICE_COMMAND_REGEX: &str = r"!\d*d\d+((k|kh|kl)\d+)?";

const MAX_QUANTITY: usize = 1000;

#[derive(Debug)]
pub struct RollResult {
    pub original_text: String,
    pub non_roll_portion: String,
    pub rolls: Vec<i32>,
    pub roll_total: u32,
    // Critical success and failure detection is determined solely by coming up with 20 or 1 on a d20 roll
    pub has_critical_success: bool,
    pub has_critical_failure: bool,
    sorting_priority: usize,
}

impl RollResult {
    pub fn new(
        original_text: String,
        non_roll_portion: String,
        rolls: Vec<i32>,
        roll_total: u32,
        has_critical_success: bool,
        has_critical_failure: bool,
        sorting_priority: usize,
    ) -> Self {
        RollResult {
            original_text,
            non_roll_portion,
            rolls,
            roll_total,
            has_critical_success,
            has_critical_failure,
            // Sort by location found in string to order from left to right, not be evaluation order
            sorting_priority,
        }
    }
}

#[derive(Debug)]
pub struct RollStringResult<'a> {
    pub original_text: &'a str,
    pub converted_text: String,
    pub rolls: Vec<RollResult>,
}

impl<'a> RollStringResult<'a> {
    pub fn new(original_text: &'a str) -> Self {
        RollStringResult {
            converted_text: String::from(original_text),
            original_text,
            rolls: vec![],
        }
    }
}

// This accepts a roll string, which is a certain amount of numbers or rolls all separated by operators
pub fn interpret_rolls<'a>(
    input: &'a str,
    input_offset: usize, // This is called recursively, keep track of where exactly we are evaluating in the string
) -> Result<RollStringResult, WakeBotError> {
    // Remove ! from beginning if it is there (legacy behavior from previously saved actions in AWS)
    let input = if input.starts_with("!") {
        &input[1..]
    } else {
        input
    };
    let mut result = RollStringResult::new(input);
    // We will have checked that it is a valid input string before invoking

    // Check for parens and recursively invoke as needed.
    let paren_regex = Regex::new(PAREN_REGEX).unwrap();

    // Target any parts of the string nested in params and replace them by recursively calling this function
    loop {
        let text_result: String;
        let start: usize;
        let end: usize;
        match paren_regex.captures(&result.converted_text) {
            Ok(Some(cap)) => {
                let nested_string = cap.get(1).unwrap();
                start = nested_string.start();
                end = nested_string.end();
                let mut nested_result = interpret_rolls(nested_string.as_str(), start).unwrap();
                text_result = nested_result.converted_text;
                result.rolls.append(&mut nested_result.rolls);
            }
            Ok(None) => break,
            Err(_) => {
                return Err(WakeBotError::new(
                    "Error occurred while parsing paren regex",
                ))
            }
        }
        result.converted_text = String::from(&result.converted_text[0..start - 1])
            + &text_result
            + &result.converted_text[end + 1..];
    }

    let roll_regex = Regex::new(ROLL_REGEX).unwrap();

    loop {
        // Invoke function that handles rolling
        match roll_regex.captures(&result.converted_text) {
            Ok(Some(cap)) => {
                let dice_count = cap.get(2).unwrap();
                let dice_count = if let Ok(n) = dice_count.as_str().parse::<usize>() {
                    n
                } else {
                    1
                };
                if dice_count > MAX_QUANTITY {
                    return Err(WakeBotError::new(&format!(
                        "Max number of dice is {}",
                        MAX_QUANTITY
                    )));
                }
                let dice_max = cap
                    .get(3)
                    .unwrap()
                    .as_str()
                    .parse::<usize>()
                    .expect("Error while parsing dice max.");
                let mut results = vec![];
                for _ in 0..dice_count {
                    let roll_result: i32 = rand::thread_rng()
                        .gen_range(1..=dice_max)
                        .try_into()
                        .expect("Negative value generated by roll");
                    results.push(roll_result);
                }
                let keep_str = cap.get(5);
                let keep_count = cap.get(6);
                let results_clone = results.clone();
                let mut removed_indices = results_clone
                    .iter()
                    .enumerate()
                    .collect::<Vec<(usize, &i32)>>();
                removed_indices.sort_unstable_by(|a, b| a.1.cmp(b.1));
                if let Some(str) = keep_str {
                    let count = keep_count
                        .expect("Keep string passed with no keep count.")
                        .as_str()
                        .parse::<i32>()
                        .expect("Invalid keep count passed.");
                    // results length limited by MAX_QUANTITY
                    let mut number_to_remove: i32 = (results.len() as i32) - count;
                    if number_to_remove < 0 {
                        number_to_remove = 0;
                    }
                    if str.as_str().eq("kl") {
                        removed_indices.reverse();
                    }
                    removed_indices.drain(number_to_remove as usize..removed_indices.len()); // Above condition ensures non-negative
                    for (i, _) in removed_indices {
                        results[i] = -results[i];
                    }
                }
                let has_critical_success = dice_max == 20 && results.iter().any(|n| *n == 20);
                let has_critical_failure = dice_max == 20 && results.iter().any(|n| *n == 1);
                let roll_total = results.iter().fold(0, |mut a, b| {
                    let n = *b;
                    if n >= 0 {
                        a += n as u32;
                    }
                    a
                });
                let dice_str = cap.get(1).unwrap();
                let (start, end) = (dice_str.start(), dice_str.end());
                let non_roll_portion = String::from(if let Some(str) = cap.get(7) {
                    str.as_str()
                } else {
                    ""
                });
                let sorting_priority = result
                    .converted_text
                    .find(cap.get(0).unwrap().as_str())
                    .unwrap()
                    + input_offset;
                // Do math here?
                result.rolls.push(RollResult::new(
                    String::from(dice_str.as_str()),
                    non_roll_portion,
                    results,
                    roll_total,
                    has_critical_success,
                    has_critical_failure,
                    sorting_priority,
                ));
                result.converted_text = String::from(&result.converted_text[0..start])
                    + &roll_total.to_string()
                    + &result.converted_text[end..];
                // We've defined the rolls, now what do we do? Probably mutate the result instance to update its string.
                // At what point do we determine what other math goes along with the roll?
            }
            Ok(None) => break,
            Err(_) => return Err(WakeBotError::new("Error occurred while parsing roll regex")),
        }
    }

    result
        .rolls
        .sort_by(|a, b| a.sorting_priority.cmp(&b.sorting_priority));

    Ok(result)
}

pub fn format_rolls_result_new(result: RollStringResult) -> String {
    let full_expr = ShuntingParser::parse_str(&result.converted_text).unwrap();
    let full_result = MathContext::new().eval(&full_expr).unwrap();
    format!(
        "{}\n{}{}**{}**",
        result.original_text.replace("*", r"\*"),
        result.rolls.iter().fold(String::from(""), |a, b| {
            // Display each roll
            let converted_text = b.roll_total.to_string() + &b.non_roll_portion;
            let expr = ShuntingParser::parse_str(&converted_text).unwrap();
            let result = MathContext::new().eval(&expr).unwrap();
            a + &format!(
                "{} ({} -> {}){} = {}{}{}\n",
                b.original_text,
                b.rolls
                    .iter()
                    .map(|&roll_num| {
                        if roll_num < 0 {
                            return String::from("~~") + &roll_num.abs().to_string() + "~~";
                        }
                        roll_num.to_string().replace("*", r"\*")
                    })
                    .collect::<Vec<String>>()
                    .join(", "),
                b.roll_total,
                b.non_roll_portion,
                result,
                if b.has_critical_success {
                    " - **CRITICAL SUCCESS!**"
                } else {
                    ""
                },
                if b.has_critical_failure {
                    " - **CRITICAL FAILURE!**"
                } else {
                    ""
                }
            )
        }),
        if result.rolls.len() > 1 {
            result.converted_text.replace("*", r"\*") + "\n"
        } else {
            String::from("")
        },
        full_result
    )
}
