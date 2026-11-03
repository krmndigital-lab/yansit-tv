# Yansıt R8 rules. The app is GPL-3.0 open source, so we shrink but never obfuscate.
-dontobfuscate
# JNI: libyansit.so binds Java_com_yansit_tv_N_* by name; keep the whole (tiny) app package.
-keep class com.yansit.tv.** { *; }
# play-services-ads 23.x references this Android 15 (API 35) framework class. It is only used on
# API 35+ devices, where the framework provides it; compileSdk 34's android.jar lacks it, so R8
# would fail with "Missing class". Drop these lines once compileSdk >= 35.
-dontwarn android.media.LoudnessCodecController
-dontwarn android.media.LoudnessCodecController$OnLoudnessCodecUpdateListener
