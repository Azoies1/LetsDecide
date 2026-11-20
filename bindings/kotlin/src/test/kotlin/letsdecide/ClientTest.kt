package letsdecide

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertTrue
import org.json.JSONObject

class ClientTest {
    private val request = JSONObject(
        """{"input": "Charged twice.", "questions": {
             "billing": {"kind": "yesno", "instructions": "Is this about billing?"},
             "team": {"kind": "choice", "instructions": "Which team?",
                      "options": {"billing": "Payments", "tech": "Bugs"}}}}""",
    )

    private fun replying(status: Int, body: String, headers: List<Pair<String, String>> = emptyList(), sent: MutableList<Http>? = null) =
        Transport { http -> sent?.add(http); HttpReply(status, headers, body) }

    @Test
    fun buildsSendsAndParses() {
        val sent = mutableListOf<Http>()
        val body = """{"answers": {"billing": {"noul": 0.9},
            "team": {"choice": "billing", "confidence": 0.8, "probabilities": {"billing": 0.8, "tech": 0.2}}},
            "usage": {"total_input_tokens": 7}}"""
        val out = Client(byProvider = mapOf("gemini" to ProviderSettings(apiKey = "k")), transport = replying(200, body, sent = sent)).decide("gemini/gemini-decision-1", request)
        assertEquals("https://generativelanguage.googleapis.com/v1beta/decisions", sent[0].url)
        assertTrue(sent[0].headers.contains("x-goog-api-key" to "k"))
        assertEquals("gemini-decision-1", JSONObject(sent[0].body).getString("model"))
        assertEquals(0.9, out.getJSONObject("answers").getJSONObject("billing").getDouble("probability"), 1e-6)
        assertEquals("billing", out.getJSONObject("answers").getJSONObject("team").getString("choice"))
        assertEquals(7, out.getInt("input_tokens"))
        assertTrue(out.has("estimated_cost_usd"))
    }

    @Test
    fun errorsAreTypedWithRetryInfo() {
        val c = Client(byProvider = mapOf("jev" to ProviderSettings(apiKey = "k")), transport = replying(429, "{}", listOf("Retry-After" to "2")))
        val e = assertFailsWith<LetsDecideException> { c.decide("jev/jev-latest", request) }
        assertEquals("RateLimited", e.code)
        assertTrue(e.retryable)
        assertEquals(2000L, e.retryAfterMs)
        assertEquals(429, e.status)
    }

    @Test
    fun invalidRequestsNeverSend() {
        val c = Client(transport = { error("must not send") })
        val bad = JSONObject("""{"input": "x", "questions": {"Bad-Id": {"kind": "yesno", "instructions": "?"}}}""")
        assertEquals("InvalidRequest", assertFailsWith<LetsDecideException> { c.decide("jev/jev-latest", bad) }.code)
    }

    @Test
    fun clefTokenIsRefreshedPerRequest() {
        val sent = mutableListOf<Http>()
        var n = 0
        val c = Client(
            byProvider = mapOf("clef" to ProviderSettings(accountId = "a", accessToken = { "tok${++n}" })),
            transport = replying(401, "{}", sent = sent),
        )
        repeat(2) { assertFailsWith<LetsDecideException> { c.decide("clef/clef-flash", request) } }
        assertEquals(listOf("Bearer tok1", "Bearer tok2"), sent.map { h -> h.headers.first { it.first == "authorization" }.second })
    }

    @Test
    fun eachProviderOnlySeesItsOwnSettings() {
        val sent = mutableListOf<Http>()
        val c = Client(
            byProvider = mapOf(
                "gemini" to ProviderSettings(apiKey = "g"),
                "jev" to ProviderSettings(headers = listOf("x-t" to "own")),
            ),
            headers = listOf("x-trace" to "t", "x-t" to "shared"),
            transport = replying(401, "{}", sent = sent),
        )
        for (m in listOf("gemini/gemini-decision-1", "jev/jev-latest")) {
            assertFailsWith<LetsDecideException> { c.decide(m, request) }
        }
        val (gemini, jev) = sent
        assertTrue(gemini.headers.contains("x-goog-api-key" to "g"))
        assertTrue(jev.headers.none { it.second.contains("g") && it.first != "x-trace" && it.first != "content-type" })
        assertTrue(jev.headers.none { it.first == "authorization" })
        assertTrue(jev.headers.contains("x-t" to "own"))
        assertTrue(jev.headers.none { it == ("x-t" to "shared") })
        assertTrue(sent.all { h -> h.headers.contains("x-trace" to "t") })
    }
}
