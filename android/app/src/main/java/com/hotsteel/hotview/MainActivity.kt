package com.hotsteel.hotview

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.core.splashscreen.SplashScreen.Companion.installSplashScreen

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        installSplashScreen()
        super.onCreate(savedInstanceState)
        // Android 15+ enforces edge-to-edge; we opt in explicitly and draw the
        // Material 3 Expressive surfaces right up to the system bars.
        enableEdgeToEdge()
        setContent {
            HotviewApp()
        }
    }
}
