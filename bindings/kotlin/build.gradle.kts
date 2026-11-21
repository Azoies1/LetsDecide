// Kotlin/JVM binding. `./gen.sh` builds the Rust library and generates the
// UniFFI glue into build/generated before Gradle compiles.
plugins {
    kotlin("jvm") version "2.0.21"
}

group = "dev.letsdecide"
version = "0.3.0"

repositories { mavenCentral() }

dependencies {
    implementation("net.java.dev.jna:jna:5.14.0")
    implementation("org.json:json:20240303")
    testImplementation(kotlin("test"))
}

// Bytecode for Java 17+, built with whatever JDK runs Gradle.
kotlin { compilerOptions { jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17) } }
java { sourceCompatibility = JavaVersion.VERSION_17; targetCompatibility = JavaVersion.VERSION_17 }

sourceSets {
    main {
        kotlin.srcDir("build/generated/uniffi")
        resources.srcDir("build/generated/native")
    }
}

tasks.test {
    useJUnitPlatform()
    // Lets JNA find the library in a dev checkout without packaging it.
    systemProperty("jna.library.path", file("../../target/release").absolutePath)
}
