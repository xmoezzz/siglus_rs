
struct PS_INPUT {
    float4 v0 : COLOR0;
};

struct PS_OUTPUT {
    float4 oC0 : COLOR0;
};

PS_OUTPUT main(PS_INPUT input) {
    PS_OUTPUT output;
    output.oC0 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r0 = float4(0.0, 0.0, 0.0, 0.0);

    r0.xyz = lerp(input.v0.xyz, c0.xyz, c0.www);
    r0.xyz = (r0.xyz + c1.xyz);
    r0.w = input.v0.w;
    output.oC0 = r0;
    return output;
}
