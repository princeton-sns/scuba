package com.example.rustskeleton

import androidx.appcompat.app.AppCompatActivity
import android.os.Bundle
import android.widget.TextView
import com.example.rustskeleton.databinding.ActivityMainBinding
import java.lang.Exception
import java.lang.Void

class MainActivity : AppCompatActivity() {

    private lateinit var binding: ActivityMainBinding

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        binding = ActivityMainBinding.inflate(layoutInflater)
        setContentView(binding.root)

        // Example of a call to a native method
        binding.sampleText.text = helloRust("Kotlin")
    }

    /**
     * A native method that is implemented by the 'rustskeleton' native library,
     * which is packaged with this application.
     */
    external fun helloRust(s: String): String

    companion object {
        init {
            System.loadLibrary("rust")
        }
    }
}