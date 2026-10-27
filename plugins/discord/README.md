# Discord

This mapper translates Discord Gateway frames and HTTP responses without doing I/O.
The host owns the socket, heartbeat timers, credentials, acceptance and delivery.

Configure a connection with this plugin, the API base URL
`https://discord.com/api/v10`, your Discord user ID as `owner`, and
`secrets = { token = "DISCORD_BOT_TOKEN" }`. Enable **Message Content Intent** in
the application's Bot settings, and install it to a server with the `bot` and
`applications.commands` scopes. Give it View Channels, Send Messages, Send
Messages in Threads, Read Message History, Attach Files and Embed Links in the
channels it should use. Direct messages require sharing a server with the bot.

`/session`, `/cancel` and `/everyone` are native application commands.
**Backtrack here** is a message context command. Typing `/cancel` as ordinary
message text does not invoke a command. Commands and Cancel buttons are deferred
privately on receipt; their result follows acceptance and authorization by the host.

Responses use Components V2, with Markdown tables shown in code blocks and image
or file attachments exposed through components. A Working card shows the latest
five tool names and their outcomes, with a Cancel button.

Gateway sessions resume across brief disconnects and restarts. Discord cannot
replay messages once a session has expired; send those messages again. The mapper
does not implement sharding, compression, user installations, modal forms, forum
topics or REST history recovery.

The bot token is placed only in the API Authorization header and in Identify or
Resume frames sent to `*.discord.gg`. Attachment downloads use the declared CDN
origin without the bot token.

Run the offline protocol tests with `cargo test --manifest-path Cargo.toml`.
They cover recorded-form Gateway events and HTTP responses; they send no messages.
Protocol references: [Gateway](https://docs.discord.com/developers/events/gateway),
[interactions](https://docs.discord.com/developers/interactions/receiving-and-responding),
[Components V2](https://docs.discord.com/developers/components/reference).
