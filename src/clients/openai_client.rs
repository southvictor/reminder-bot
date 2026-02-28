use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use chrono_tz::{America::New_York, Tz};
use reqwest;

#[derive(Debug, Serialize, Deserialize)]
struct OpenAIMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    max_tokens: u32,
    temperature: f32,
}

#[derive(Debug, Deserialize)]
struct OpenAIResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Debug, Deserialize)]
struct Message {
    content: String,
}

fn normalize_timezone(timezone: &str) -> Tz {
    timezone.parse::<Tz>().unwrap_or(New_York)
}

pub async fn generate_openai_prompt(
    prompt: &str,
    prompt_type: &str,
    timezone: &str,
    api_key: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let now: DateTime<Utc> = Utc::now();
    let tz = normalize_timezone(timezone);
    let now_local = now.with_timezone(&tz).to_rfc3339();
    let tz_name = tz.name();

    let full_prompt = match prompt_type {
        "calendar_event_parser" => format!(
            "You are a calendar event extraction engine.\n\
             Current date and time ({tz_name}): {now}\n\
             User timezone: {tz_name}\n\
             Task: From the user message below, extract:\n\
             - \"content\": the core calendar event text with extraneous scheduling words removed. For example:\n\
               - \"buy eggs tomorrow\" -> \"buy eggs\"\n\
               - \"notify me to call mom at 5\" -> \"call mom\"\n\
             - \"time\": an RFC3339 datetime string in the user's timezone.\n\
             Rules:\n\
             - If the user gives an explicit date like \"December 6th\", use that exact month and day at noon in the local timezone; do NOT change them.\n\
             - If the year is omitted, assume the next occurrence of that date on or after the current date.\n\
             - If the user gives a relative time (e.g. \"in two weeks\", \"tomorrow at 3pm\"), compute the concrete datetime from the current date/time.\n\
             - For day-of-week phrases:\n\
               - \"Saturday\" or \"this Saturday\" means the next occurrence of that weekday on or after today.\n\
               - \"next Saturday\" means the occurrence in the following week (at least 7 days after today), not the immediate upcoming one.\n\
             - If the time expression is unclear or missing (e.g. \"soon\", \"later\"), set the time to exactly 24 hours after the current datetime.\n\
             - If the user includes corrections or clarifications (e.g. \"actually I meant this Saturday\"), treat them as time corrections only and DO NOT include them in \"content\".\n\
             - If the message contains a \"Context notes\" or \"Additional context\" section, never copy that text into \"content\".\n\
             - Never invent or adjust the date away from what the user wrote; only add a year or time if needed.\n\
             - Output ONLY raw JSON, no prose, markdown, or code fences.\n\
             - The JSON shape must be exactly:\n\
             {{\"content\":\"<string>\",\"time\":\"<RFC3339 datetime>\"}}\n\
             User message: \"{user_prompt}\"",
            tz_name = tz_name,
            now = now_local,
            user_prompt = prompt
        ),
        "calendar_event_correction" => format!(
            "You are a calendar event correction engine.\n\
             Current date and time ({tz_name}): {now}\n\
             User timezone: {tz_name}\n\
             Task: Given the original calendar event request and a user-provided correction note, output a corrected calendar event.\n\
             Rules:\n\
             - The correction note is NOT calendar event content. It is only for fixing the date/time or clarifying intent.\n\
             - Preserve the original calendar event content unless the correction explicitly changes it.\n\
             - If the correction only adjusts time (e.g. \"actually I meant this Saturday\"), update only the time.\n\
             - Output ONLY raw JSON, no prose, markdown, or code fences.\n\
             - The JSON shape must be exactly:\n\
             {{\"content\":\"<string>\",\"time\":\"<RFC3339 datetime>\"}}\n\
             Original request: \"{user_prompt}\"",
            tz_name = tz_name,
            now = now_local,
            user_prompt = prompt
        ),
        "calendar_event_message" => format!(
            "You are a calendar event message formatter.\n\
             Current date and time ({tz_name}): {now}\n\
             Task: Given the structured calendar event info below, write a short, natural English calendar event message to send to a user.\n\
             Rules:\n\
             - Address the user(s) in second person (\"you\").\n\
             - Mention the event time explicitly in the user's timezone.\n\
             - Include the calendar event content naturally.\n\
             - If hours remaining is provided, include it in a friendly way.\n\
             - Keep it to 1–2 sentences, no markdown, no lists, no JSON.\n\
             - Do NOT wrap the output in quotes.\n\
             Structured input:\n\
             {structured}",
            tz_name = tz_name,
            now = now_local,
            structured = prompt
        ),
        "intent_router" => format!(
            "You are an intent router for a helper (jarvis from iron man) bot.\n\
             I want you do classify the user's message into one of these intents:\n\
             - calendar_event: user wants to be reminded about a specific event at a specific time\n\
             - todolist: user wants to keep track of tasks that they need to do at some unspecified time\n\
             - config: requests to change system configuration (e.g., timezone)\n\
             - unknown: unclear or missing time/action\n\
             Output ONLY raw JSON, no prose, markdown, or code fences.\n\
             The JSON shape must be exactly:\n\
             {{\"intent\":\"calendar_event or todolist or config or unknown\"}}\n\
             User message: \"{user_prompt}\"",
            user_prompt = prompt
        ),
        "config_parser" => format!(
            r#"You are a configuration parser for a calendar event bot.
Task: Extract a configuration change from the user message.
Supported config kinds:
- timezone
Rules:
- If the user says Eastern time, map to America/New_York.
- If the user says Central time, map to America/Chicago.
- If the user says Mountain time, map to America/Denver.
- If the user says Pacific time, map to America/Los_Angeles.
- If they give a valid IANA timezone, keep it.
- If unclear, default to America/New_York.
Output ONLY raw JSON, no prose, markdown, or code fences.
The JSON shape must be exactly:
{{"kind":"timezone","value":"<IANA timezone>"}}
User message: "{user_prompt}""#,
            user_prompt = prompt
        ),
        _ => return Err("Not a valid base prompt".to_string().into()),
    };

    query_openai(full_prompt, prompt_type, api_key).await
}

async fn query_openai(
    prompt: String,
    prompt_type: &str,
    api_key: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let system_message = match prompt_type {
        "calendar_event_parser" | "calendar_event_correction" => {
            "You are a strict JSON calendar event extraction engine. You read instructions and a user message and reply ONLY with a single JSON object, with no markdown, no backticks, and no extra text. If the user gives an explicit date (e.g. \"December 6th\"), you preserve that exact month and day and only fill in missing year/time according to the instructions."
        }
        "intent_router" | "config_parser" | "todo_parser" => {
            "You are a strict JSON router. Reply ONLY with a single JSON object, with no markdown, no backticks, and no extra text."
        }
        "calendar_event_message" => {
            "You are a calendar event message formatter. Reply with plain text only (no JSON, no markdown, no quotes)."
        }
        _ => "You are a helpful assistant.",
    };

    let request: OpenAIRequest = OpenAIRequest {
        model: "gpt-4o".to_string(),
        messages: vec![
            OpenAIMessage {
                role: "system".to_string(),
                content: system_message.to_string(),
            },
            OpenAIMessage {
                role: "user".to_string(),
                content: prompt,
            },
        ],
        max_tokens: 1500,
        temperature: 0.2,
    };

    let client = reqwest::Client::new();
    let response = client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&request)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("OpenAI API error: {}: {}", status, body).into());
    }

    let response_body: OpenAIResponse = response.json().await?;
    let Some(choice) = response_body.choices.first() else {
        return Err("OpenAI API returned no choices".into());
    };
    Ok(choice.message.content.clone())
}
