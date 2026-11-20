package letsdecide

import java.net.URI
import java.net.http.HttpClient
import java.net.http.HttpRequest
import java.net.http.HttpResponse
import java.time.Duration
import org.json.JSONArray
import org.json.JSONObject

/** A typed error from any provider. Never carries keys or request bodies. */
class LetsDecideException(
    val code: String,
    message: String,
    val status: Int? = null,
    val retryAfterMs: Long? = null,
    val retryable: Boolean = false,
) : Exception(message)

/** The request the core built; the transport sends it. */
data class Http(val method: String, val url: String, val headers: List<Pair<String, String>>, val body: String)

/** The provider's reply. Header names are compared case-insensitively. */
data class HttpReply(val status: Int, val headers: List<Pair<String, String>>, val body: String)

/** Sends one request. Swap it for proxies, pinning or tests. */
fun interface Transport {
    fun send(http: Http): HttpReply
}

/** `java.net.http`, 60 s timeout (decisions on media can be slow). */
class JdkTransport(private val client: HttpClient = HttpClient.newHttpClient()) : Transport {
    override fun send(http: Http): HttpReply {
        val req = HttpRequest.newBuilder(URI.create(http.url))
            .timeout(Duration.ofSeconds(60))
            .method(http.method, HttpRequest.BodyPublishers.ofString(http.body))
        // `content-length` and `host` are set by the JDK.
        http.headers.forEach { (k, v) -> req.header(k, v) }
        val res = client.send(req.build(), HttpResponse.BodyHandlers.ofString())
        val headers = res.headers().map().flatMap { (k, vs) -> vs.map { k to it } }
        return HttpReply(res.statusCode(), headers, res.body())
    }
}

/**
 * One provider's settings. A provider only ever sees its own entry, so a key
 * can never reach another provider.
 *
 * @param accessToken OAuth token provider (Clef), called on every request so
 *   the host can refresh it. Wins over [apiKey].
 * @param headers Headers for this provider only; win over the client's shared
 *   headers with the same name.
 */
class ProviderSettings(
    val apiKey: String? = null,
    val accessToken: (() -> String)? = null,
    val baseUrl: String? = null,
    val accountId: String? = null,
    val headers: List<Pair<String, String>> = emptyList(),
)

/**
 * One request shape for every decision API. Requests and responses are
 * `JSONObject`s in the canonical shape (see the README).
 *
 * @param byProvider Settings per provider prefix, e.g.
 *   `mapOf("gemini" to ProviderSettings(apiKey = key))`.
 * @param headers Sent to every provider (tracing, gateways). Never secrets.
 */
class Client(
    private val byProvider: Map<String, ProviderSettings> = emptyMap(),
    private val headers: List<Pair<String, String>> = emptyList(),
    private val transport: Transport = JdkTransport(),
) {
    /** `model`: `"provider/model"`, e.g. `"clef/clef-flash"`. Throws [LetsDecideException]. */
    fun decide(model: String, request: JSONObject): JSONObject {
        val req = request.toString()
        val own = byProvider[model.substringBefore('/')] ?: ProviderSettings()
        val names = own.headers.map { it.first.lowercase() }.toSet()
        val merged = headers.filter { it.first.lowercase() !in names } + own.headers
        val config = JSONObject()
            .putOpt("api_key", own.apiKey)
            .putOpt("access_token", own.accessToken?.invoke())
            .putOpt("base_url", own.baseUrl)
            .putOpt("account_id", own.accountId)
            .put("headers", JSONArray(merged.map { JSONArray(listOf(it.first, it.second)) }))
        val built = unwrap(letsdecide.ffi.build(model, req, config.toString()))
        val sentHeaders = built.getJSONArray("headers").map { (it as JSONArray).let { h -> h.getString(0) to h.getString(1) } }
        val reply = transport.send(Http(built.getString("method"), built.getString("url"), sentHeaders, built.getString("body")))
        val res = JSONObject()
            .put("status", reply.status)
            .put("headers", JSONArray(reply.headers.map { JSONArray(listOf(it.first, it.second)) }))
            .put("body", reply.body)
        return unwrap(letsdecide.ffi.parse(model, req, res.toString()))
    }

    // The core returns {ok} or {error}; unwrap or throw.
    private fun unwrap(json: String): JSONObject {
        val out = JSONObject(json)
        val e = out.optJSONObject("error") ?: return out.getJSONObject("ok")
        throw LetsDecideException(
            code = e.getString("code"),
            message = e.getString("message"),
            status = if (e.isNull("status")) null else e.getInt("status"),
            retryAfterMs = if (e.isNull("retry_after_ms")) null else e.getLong("retry_after_ms"),
            retryable = e.optBoolean("retryable"),
        )
    }
}
