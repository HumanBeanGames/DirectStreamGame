# Human Bean Games Community Roadmap

This project now has the core loop needed for a playable web-native stream game:

- Bevy game server runs locally.
- Cloudflare Tunnel exposes the live backend.
- Cloudflare Pages serves an always-available player shell.
- Viewers can watch the stream, hear audio, chat, and trigger commands.
- The main website can embed the standalone stream page.

## Recommended Next Layers

### 1. Moderation

Add moderation before chat commands reach gameplay systems.

- Per-user/IP-hash rate limits.
- Command cooldowns.
- Blocked generated usernames or identity hashes.
- Banned phrase filtering.
- Message length limits.
- Moderator/admin-only commands.
- Stats-window controls for purge, mute, ban, and unban.
- Optional moderation log.

### 2. Redeemable Codes

Add a simple code redemption system before full account login.

- Endpoint such as `/redeem-code`.
- Code maps to a reward type.
- Reward applies to the current generated chat identity.
- Optional expiry, use limit, or one-use-per-user rules.
- Codes can be shared through Patreon, Substack, Discord, or newsletters.

Example rewards:

- Special chat badge.
- Temporary chat color.
- Cosmetic game effect.
- Bonus command.
- Sound effect unlock.
- Vote weight.
- Daily item or spawn token.

### 3. Real Usernames

Start lightweight before adding full authentication.

- Optional display-name input in the player.
- Store the chosen name against the active identity hash.
- Reserve protected names.
- Prevent duplicate active names.
- Add simple rename cooldowns.

Later login options:

- Twitch login.
- Discord login.
- Patreon OAuth.
- Email magic link.

### 4. Supporter Integrations

Patreon is the strongest fit for automated supporter status. Substack is likely
better as a code-distribution channel unless its API/login flow fits the site.

Possible supporter flow:

- Viewer logs in with Patreon.
- Server checks membership tier.
- Player shows supporter badge/perks.
- Game systems receive supporter metadata alongside chat commands.

Simpler code-based flow:

- Post a code in Patreon/Substack.
- Viewer enters the code in the player.
- The backend grants a session or persistent reward.

### 5. Website Polish

The landing page can grow into a proper game hub.

- Embedded game/player.
- Standalone play link.
- "How to play" command list.
- Current online/offline status.
- Schedule.
- Archive/clips.
- Patreon/Substack links.
- Credits and supporter list.

## Suggested Order

1. Moderation basics.
2. Redeemable codes.
3. Optional display names.
4. Website polish.
5. Patreon/Substack integration.
6. Full account login.
