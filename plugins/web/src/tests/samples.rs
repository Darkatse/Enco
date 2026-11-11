//! Answers from the services the plugin reads; each says where it came from.

/// A search response in the form Exa's schema gives. The second result is the untitled one Exa
/// returned on 2026-11-10 for a search about Google Cloud's free trial, as the Agent recorded it;
/// the rest is illustrative.
pub(super) const EXA_SEARCH: &str = r#"{
  "requestId": "6b1f9a2c4d7e8f0a1b2c3d4e5f607182",
  "results": [
    {
      "id": "https://cloud.google.com/free",
      "title": "Free Trial and Free Tier Services and Products | Google Cloud",
      "url": "https://cloud.google.com/free",
      "publishedDate": "2026-11-04T00:00:00.000Z",
      "author": null,
      "highlights": [
        "New customers get $300 in free credits to fully explore Google Cloud.",
        "All customers get free usage of 20+ products, up to monthly limits."
      ]
    },
    {
      "id": "https://docs.cloud.google.com/free/docs/free-cloud-features",
      "title": "",
      "url": "https://docs.cloud.google.com/free/docs/free-cloud-features",
      "publishedDate": "2026-11-10T00:00:00.000Z"
    }
  ],
  "costDollars": {"total": 0.005}
}"#;

/// A contents response in the form Exa's schema gives, for a page Exa could not retrieve.
pub(super) const EXA_CONTENTS_FAILED: &str = r#"{
  "requestId": "0c9d8e7f6a5b4c3d2e1f0a9b8c7d6e5f",
  "results": [],
  "statuses": [
    {
      "id": "https://example.com/does-not-exist-enco-probe",
      "status": "error",
      "error": {"tag": "CRAWL_NOT_FOUND", "httpStatusCode": 404}
    }
  ]
}"#;

/// What Jina Reader answered on 2026-11-11 for an image, with a blank warning.
pub(super) const JINA_IMAGE: &str = r#"{"code":200,"status":20000,"data":{"title":"w3c-72x48.png","description":"","url":"https://www.w3.org/Icons/w3c_home.png","finalUrl":"https://www.w3.org/assets/logos/w3c-2025-transitional/w3c-72x48.png","content":"A 72x48 small image, likely a logo, icon or avatar","publishedTime":"Fri, 24 Oct 2025 15:06:28 GMT","warning":"","httpStatus":200,"httpStatusText":"","usage":{"outputTokens":15,"measuredTokens":15,"scaledTokens":15,"tokens":15}},"meta":{"usage":{"outputTokens":15,"measuredTokens":15,"scaledTokens":15,"tokens":15}}}"#;

/// What Jina Reader answered on 2026-11-11 for a page that does not exist.
pub(super) const JINA_NOT_FOUND: &str = r#"{"code":200,"status":20000,"data":{"title":"Example Domain","description":"","url":"https://example.com/does-not-exist-enco-probe","finalUrl":"https://example.com/does-not-exist-enco-probe","content":"This domain is for use in documentation examples without needing permission. This is not a service; avoid relying on it for testing and monitoring purposes.\n\nهذا النطاق مُخصص للاستخدام في أمثلة التوثيق دون الحاجة إلى إذن. هذه ليست خدمة، يُرجى تجنب الاعتماد عليها لأغراض الاختبار والمراقبة.\n\n该域名仅用于文档示例，无需获得许可。这并非一项服务，请勿将其用于测试和监控目的。\n\nL’usage de ce domaine est réservé à des exemples de documentation, sans autorisation préalable. Il ne s’agit pas d’un service ; son utilisation à des fins de test ou de surveillance est à éviter.\n\nДанный домен предназначен для использования в примерах документации без необходимости получения предварительного разрешения. Это не сервис; не рекомендуется его использование для тестирования и мониторинга.\n\nEste dominio está destinado al uso en ejemplos de documentación sin necesidad de permiso. Esto no es un servicio; evitar utilizarlo para realizar pruebas o monitoreos.\n\n[Learn more](https://iana.org/help/example-domains)","warning":"Target URL returned error 404: Not Found","metadata":{"lang":"en","viewport":"width=device-width,initial-scale=1"},"external":{"icon":{"data:,":{}}},"httpStatus":404,"httpStatusText":"","usage":{"outputTokens":341,"measuredTokens":341,"scaledTokens":341,"tokens":341}},"meta":{"usage":{"outputTokens":341,"measuredTokens":341,"scaledTokens":341,"tokens":341}}}"#;
