# JNI: the class name is looked up from C++, and the callbacks are invoked
# through JNI reflection, so keep both intact.
-keep class com.hotsteel.hotview.native.NativeBridge { *; }
-keep interface com.hotsteel.hotview.native.NativeMediaEvents { *; }
-keep class * implements com.hotsteel.hotview.native.NativeMediaEvents { *; }
-keepclasseswithmembernames class * {
    native <methods>;
}
