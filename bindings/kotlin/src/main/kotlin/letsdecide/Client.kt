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
 * One request shape for every decision API. Requests and responses are
 * `JSONObject`s in the canonical shape (see the README).
 *
 * @param accessToken OAuth token provider (Clef), called on every request so
 *   the host can refresh it. Wins over `apiKey`.
 */
class Client(
    private val apiKey: String? = null,
    private val accessToken: (() -> String)? = null,
    private val baseUrl: String? = null,
    private val accountId: String? = null,
    private val transport: Transport = JdkTransport(),
) {
    /** `model`: `"provider/model"`, e.g. `"clef/clef-flash"`. Throws [LetsDecideException]. */
    fun decide(model: String, request: JSONObject): JSONObject {
        val req = request.toString()
        val config = JSONObject()
            .putOpt("api_key", apiKey)
            .putOpt("access_token", accessToken?.invoke())
            .putOpt("base_url", baseUrl)
            .putOpt("account_id", accountId)
        val built = unwrap(letsdecide.ffi.build(model, req, config.toString()))
        val headers = built.getJSONArray("headers").map { (it as JSONArray).let { h -> h.getString(0) to h.getString(1) } }
        val reply = transport.send(Http(built.getString("method"), built.getString("url"), headers, built.getString("body")))
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
