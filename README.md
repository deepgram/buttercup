# buttercup

Inspired by `bubbles` (https://github.com/deepgram/bubbles), `buttercup` is a server written entirely in Rust
to allow you to chat with a chatbot on the phone via the Twilio streaming API (https://www.twilio.com/docs/voice/twiml/stream).
It uses Deepgram for speech-to-text and Polly for text-to-speech.

I will be building and tagging the following out:

- [ ] Echo server
- [ ] Cleverbot
- [ ] Chat GPT

After the Chat GPT implementation has been completed, I intend to fork this repo in order to build more opinionated apps.

## Running the server

You can spin up the server by simply running `cargo run`. However, you will need the following environment variables set:

* `DEEPGRAM_API_KEY`: a Deepgram API Key to enable transcription
* `TWILIO_PHONE_NUMBER`: your Twilio phone number using a TwiML Bin set to stream to this server
* `AWS_REGION`: the AWS region to use for Polly (`us-west-2` should be fine)
* `AWS_ACCESS_KEY_ID`: AWS Key ID for Polly
* `AWS_SECRET_ACCESS_KEY`: AWS Secret Access Key for Polly
