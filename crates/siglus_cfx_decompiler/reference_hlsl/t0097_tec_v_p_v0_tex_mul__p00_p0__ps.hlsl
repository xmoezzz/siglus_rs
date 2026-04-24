uniform sampler2D s0;

static const float4 c0 = float4(-1.0, 1.0, 0.0, 0.0);

struct PS_INPUT {
    float4 v0 : COLOR0;
    float4 t0 : TEXCOORD0;
};

struct PS_OUTPUT {
    float4 oC0 : COLOR0;
};

PS_OUTPUT main(PS_INPUT input) {
    PS_OUTPUT output;
    output.oC0 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r0 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r1 = float4(0.0, 0.0, 0.0, 0.0);

    r0 = tex2D(s0, input.t0.xy);
    r0.xyz = (input.v0.xyz * r0.xyz + c0.xxx);
    r1.w = (r0.w * input.v0.w);
    r1.xyz = (r1.www * r0.xyz + c0.yyy);
    output.oC0 = r1;
    return output;
}
