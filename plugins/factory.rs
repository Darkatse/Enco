//! Host-reserved installations shared by factory builds and application embedding.
/// The factory plugin that composes for profiles naming no composer, and in safe mode.
pub const COMPOSER: &str = "composer";
/// Name and stable identity of each plugin the host ships, as an artifact and as source.
pub const PLUGINS: [(&str, &str); 9] = [
    ("openai-compatible", "01M3X4HYHSE2M3523YK35VX60W"),
    ("gemini", "01K8JQ4M267A8Y6C9N0P3R5TVX"),
    ("deepseek", "01M3X4HYHSRXVVQYXVDK5WQ9D3"),
    ("typesafe", "01M4EQVYF70Z6WGYFDKD20QF8S"),
    (COMPOSER, "01M5TKQCBXJHD1X8JQPFDVJDDJ"),
    ("web", "01K7BH08Y1C0E40KZ27QNVC89W"),
    ("telegram", "01M5VEGMRHNGRPYAM6TNKCFK4C"),
    ("discord", "01K8KC9H3MS2JVFAG5DRXTEWPN"),
    ("onebot", "01M62C3PJW8VT9GY6SRNE4B5KH"),
];
