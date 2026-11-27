package com.localtodo.mobile

import android.os.Bundle
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
    }

    override fun onResume() {
        super.onResume()
        ReminderScheduler.onActivityResumed(this)
    }

    override fun onPause() {
        ReminderScheduler.onActivityPaused(this)
        super.onPause()
    }
}
