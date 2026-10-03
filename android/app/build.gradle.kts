import com.android.build.api.dsl.SigningConfig
import java.util.Properties
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.compose.compiler)
}

// ----------------------------------------------------------------------------
// Signing: two keystores (release + debug), passwords/alias default to
// "hotsteel". Values can come from android/keystore.properties or environment
// variables; CI decodes the keystores from repository secrets if present.
// ----------------------------------------------------------------------------
val signingProperties = Properties().apply {
    val file = rootProject.file("keystore.properties")
    if (file.exists()) {
        file.inputStream().use { load(it) }
    }
}

fun signingValue(property: String, env: String, fallback: String? = null): String? =
    signingProperties.getProperty(property) ?: System.getenv(env) ?: fallback

fun SigningConfig.applyKeystore(pathProperty: String, pathEnv: String) {
    val path = signingValue(pathProperty, pathEnv) ?: return
    val file = rootProject.file(path)
    if (!file.exists()) {
        logger.lifecycle("Signing keystore not found, skipping: $path")
        return
    }
    storeFile = file
    storePassword = signingValue("${pathProperty}Password", "HOTVIEW_KEYSTORE_PASSWORD", "hotsteel")
    keyAlias = signingValue("${pathProperty}Alias", "HOTVIEW_KEY_ALIAS", "hotsteel")
    keyPassword = signingValue("${pathProperty}KeyPassword", "HOTVIEW_KEY_PASSWORD", "hotsteel")
}

android {
    namespace = "com.hotsteel.hotview"
    compileSdk = 37

    defaultConfig {
        applicationId = "com.hotsteel.hotview"
        minSdk = 26
        targetSdk = 37
        versionCode = 1
        versionName = "1.0.0"

        ndk {
            abiFilters += listOf("arm64-v8a", "armeabi-v7a", "x86_64")
        }
    }

    signingConfigs {
        create("release") {
            applyKeystore("storeFile", "HOTVIEW_KEYSTORE")
        }
        create("debugKey") {
            applyKeystore("debugStoreFile", "HOTVIEW_DEBUG_KEYSTORE")
        }
    }

    buildFeatures {
        compose = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
            // Prefer the release keystore; fall back to debug so CI without
            // secrets still produces an installable APK.
            signingConfig = if (signingConfigs.getByName("release").storeFile != null) {
                signingConfigs.getByName("release")
            } else {
                signingConfigs.getByName("debug")
            }
        }
        debug {
            signingConfig = if (signingConfigs.getByName("debugKey").storeFile != null) {
                signingConfigs.getByName("debugKey")
            } else {
                signingConfigs.getByName("debug")
            }
        }
    }

    packaging {
        resources {
            excludes += "/META-INF/{AL2.0,LGPL2.1}"
        }
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(JvmTarget.JVM_17)
    }
}

// ----------------------------------------------------------------------------
// Rust renderer / decoder (cargo-ndk). Skipped with -PskipRust=true when only
// the Kotlin layer needs checking.
// ----------------------------------------------------------------------------
val rustJniLibs = layout.buildDirectory.dir("rustJniLibs")
val skipRust = providers.gradleProperty("skipRust").orNull?.toBoolean() ?: false
if (!skipRust) {
    val cargoBuild = tasks.register<Exec>("cargoBuild") {
        group = "rust"
        description = "Builds the Hotview native renderer for all Android ABIs"
        workingDir = rootProject.file("rust")
        commandLine(
            "cargo", "ndk",
            "-t", "arm64-v8a",
            "-t", "armeabi-v7a",
            "-t", "x86_64",
            "-o", rustJniLibs.get().asFile.absolutePath,
            "build", "--release",
        )
    }
    tasks.matching { it.name == "preBuild" }.configureEach { dependsOn(cargoBuild) }
    android.sourceSets.getByName("main").jniLibs.srcDir(rustJniLibs.get().asFile)
}

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.core.splashscreen)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.runtime.ktx)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.ui.graphics)
    implementation(libs.compose.ui.tooling.preview)
    implementation(libs.compose.foundation)
    implementation(libs.compose.material3)
    implementation(libs.compose.material.icons.extended)
    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.haze)
    implementation(libs.haze.blur)
    debugImplementation(libs.compose.ui.tooling)
}
