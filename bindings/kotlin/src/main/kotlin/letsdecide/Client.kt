package letsdecide

import java.net.URI
import java.net.http.HttpClient
import java.net.http.HttpRequest
import java.net.http.HttpResponse
import java.time.Duration
import org.json.JSONArray
import org.json.JSONObject

/**
 * Why a request failed, safe to log: names, ids, counts, MIME types. Never keys
 * or content. Fields the core doesn't set are `null`.
 */
data class ErrorDetails(
    val reason: String,
    val provider: String? = null,
    val question: String? = null,
    val id: String? = null,
    val limit: Long? = null,
    val actual: Long? = null,
    val mimeType: String? = null,
    val field: String? = null,
)

/** A typed error from any provider. Never carries keys or request bodies. */
class LetsDecideException(
    val code: String,
    message: String,
    val status: Int? = null,
    val retryAfterMs: Long? = null,
    val retryable: Boolean = false,
    val details: ErrorDetails? = null,
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
 * @param options Provider-specific values, e.g. for a dialect's
 *   `{options.<name>}` path placeholders.
 */
class ProviderSettings(
    val apiKey: String? = null,
    val accessToken: (() -> String)? = null,
    val baseUrl: String? = null,
    val accountId: String? = null,
    val headers: List<Pair<String, String>> = emptyList(),
    val options: Map<String, Any?> = emptyMap(),
)

/** Built-in provider names; dialects can't take them without `replace`. */
val BUILTIN: List<String> by lazy { unwrapArray(letsdecide.ffi.builtins()).map { it as String } }

/** Capabilities of a built-in `"provider/model"`. */
fun capabilities(model: String): JSONObject = unwrap(letsdecide.ffi.capabilities(model))

/** Validates [request] and, if given, checks it against [capabilities]. Throws [LetsDecideException]. */
fun validate(request: JSONObject, capabilities: JSONObject? = null) {
    unwrapAny(letsdecide.ffi.validate(request.toString(), capabilities?.toString() ?: "null"))
}

/** Checks a canonical [response] against [request]; returns it with defaults filled. */
fun check(request: JSONObject, response: JSONObject): JSONObject =
    unwrap(letsdecide.ffi.check(request.toString(), response.toString()))

/** The error a built-in adapter would raise for this non-2xx reply. Throw it to raise it. */
fun classifyHttp(reply: HttpReply): LetsDecideException =
    toException(unwrap(letsdecide.ffi.classifyHttp(replyJson(reply).toString())))

/** A built-in dialect (`jev`, `clef`, `gemini`, `openai`, `systemone`) as JSON, to read or extend. */
fun dialect(name: String): JSONObject = unwrap(letsdecide.ffi.dialect(name))

/** Validates a dialect and expands `extends`. Throws [LetsDecideException] on a bad one. */
fun resolveDialect(spec: JSONObject): JSONObject = unwrap(letsdecide.ffi.resolveDialect(spec.toString()))

private val NAME = Regex("[a-z0-9_-]{1,64}")

/**
 * One request shape for every decision API. Requests and responses are
 * `JSONObject`s in the canonical shape (see the README).
 *
 * @param byProvider Settings per provider prefix, e.g.
 *   `mapOf("gemini" to ProviderSettings(apiKey = key))`.
 * @param headers Sent to every provider (tracing, gateways). Never secrets.
 * @param dialects Providers described as data (see docs/dialects.md), e.g.
 *   `mapOf("acme" to JSONObject("""{"extends": "jev", "base_url": "https://api.acme.ai"}"""))`.
 *   Validated here.
 * @param replace Built-in names a dialect may override (e.g. a patched `gemini`).
 */
class Client(
    private val byProvider: Map<String, ProviderSettings> = emptyMap(),
    private val headers: List<Pair<String, String>> = emptyList(),
    private val transport: Transport = JdkTransport(),
    dialects: Map<String, JSONObject> = emptyMap(),
    replace: Set<String> = emptySet(),
) {
    // Resolved once; the core takes the resolved form on every call.
    private val dialects: Map<String, String> = dialects.mapValues { (name, spec) ->
        if (!NAME.matches(name)) {
            throw invalid("provider names must match `[a-z0-9_-]{1,64}`", ErrorDetails("invalid_config", field = "name"))
        }
        if (name in BUILTIN && name !in replace) {
            throw invalid(
                "provider `$name` is already registered; list it in `replace` to override it",
                ErrorDetails("name_taken", provider = name),
            )
        }
        tagged(name) { resolveDialect(spec).toString() }
    }

    /** Capabilities of any `"provider/model"`, built-in or dialect. */
    fun capabilities(model: String): JSONObject {
        val d = dialects[model.substringBefore('/')] ?: return letsdecide.capabilities(model)
        return JSONObject(d).getJSONObject("capabilities")
    }

    /** `model`: `"provider/model"`, e.g. `"clef/clef-flash"`. Throws [LetsDecideException]. */
    fun decide(model: String, request: JSONObject): JSONObject {
        val req = request.toString()
        val name = model.substringBefore('/')
        val config = config(name).toString()
        val d = dialects[name]
        if (d != null) {
            val rest = model.substringAfter('/', "")
            if (rest.isEmpty()) throw invalid("model must look like `provider/model`", null)
            return tagged(name) {
                val built = unwrap(letsdecide.ffi.buildDialect(d, rest, req, config))
                unwrap(letsdecide.ffi.parseDialect(d, req, send(built).toString()))
            }
        }
        val built = unwrap(letsdecide.ffi.build(model, req, config))
        return unwrap(letsdecide.ffi.parse(model, req, send(built).toString()))
    }

    private fun config(name: String): JSONObject {
        val own = byProvider[name] ?: ProviderSettings()
        // The provider's own headers win over shared ones with the same name.
        val names = own.headers.map { it.first.lowercase() }.toSet()
        val merged = headers.filter { it.first.lowercase() !in names } + own.headers
        return JSONObject()
            .putOpt("api_key", own.apiKey)
            .putOpt("access_token", own.accessToken?.invoke())
            .putOpt("base_url", own.baseUrl)
            .putOpt("account_id", own.accountId)
            .put("headers", JSONArray(merged.map { JSONArray(listOf(it.first, it.second)) }))
            .put("options", JSONObject(own.options))
    }

    private fun send(built: JSONObject): JSONObject {
        val sent = built.getJSONArray("headers").map { (it as JSONArray).let { h -> h.getString(0) to h.getString(1) } }
        return replyJson(transport.send(Http(built.getString("method"), built.getString("url"), sent, built.getString("body"))))
    }
}

private fun replyJson(reply: HttpReply) = JSONObject()
    .put("status", reply.status)
    .put("headers", JSONArray(reply.headers.map { JSONArray(listOf(it.first, it.second)) }))
    .put("body", reply.body)

private fun invalid(message: String, details: ErrorDetails?) =
    LetsDecideException("InvalidRequest", message, details = details)

/** Names the provider in errors from a dialect, as built-ins do. */
private inline fun <T> tagged(name: String, f: () -> T): T = try {
    f()
} catch (e: LetsDecideException) {
    val d = e.details
    if (d == null || d.provider != null) throw e
    throw LetsDecideException(e.code, e.message ?: "", e.status, e.retryAfterMs, e.retryable, d.copy(provider = name))
}

private fun toException(e: JSONObject) = LetsDecideException(
    code = e.getString("code"),
    message = e.getString("message"),
    status = if (e.isNull("status")) null else e.getInt("status"),
    retryAfterMs = if (e.isNull("retry_after_ms")) null else e.getLong("retry_after_ms"),
    retryable = e.optBoolean("retryable"),
    details = e.optJSONObject("details")?.let { d ->
        fun str(k: String) = if (d.isNull(k)) null else d.getString(k)
        fun num(k: String) = if (d.isNull(k)) null else d.getLong(k)
        ErrorDetails(d.getString("reason"), str("provider"), str("question"), str("id"), num("limit"), num("actual"), str("mime_type"), str("field"))
    },
)

// The core returns {ok} or {error}; unwrap or throw.
private fun unwrapAny(json: String): Any? {
    val out = JSONObject(json)
    out.optJSONObject("error")?.let { throw toException(it) }
    return out.opt("ok")
}

private fun unwrap(json: String): JSONObject = unwrapAny(json) as JSONObject

private fun unwrapArray(json: String): JSONArray = unwrapAny(json) as JSONArray
