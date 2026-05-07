cmake_minimum_required(VERSION 3.22.1)

project(siglus_jni)

add_library(siglus_jni SHARED
        siglus_jni.cpp
)

find_library(android-lib android)
find_library(log-lib log)
find_library(dl-lib dl)

target_compile_features(siglus_jni PRIVATE cxx_std_17)
target_link_libraries(siglus_jni
        ${android-lib}
        ${log-lib}
        ${dl-lib}
)
