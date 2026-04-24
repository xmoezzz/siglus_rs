uniform sampler2D s0;
uniform sampler2D s1;

static const float4 c0 = float4(-1.0, 1.0, 0.0, 0.0);

struct PS_INPUT {
    float4 v0 : COLOR0;
    float4 t0 : TEXCOORD0;
    float4 t1 : TEXCOORD1;
};

struct PS_OUTPUT {
    float4 oC0 : COLOR0;
};

PS_OUTPUT main(PS_INPUT input) {
    PS_OUTPUT output;
    output.oC0 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r0 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r1 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r2 = float4(0.0, 0.0, 0.0, 0.0);

    r0 = tex2D(s0, input.t0.xy);
    r1 = tex2D(s1, input.t1.xy);
    r0.xyz = (input.v0.xyz * r0.xyz + c0.xxx);
    r2.w = (r0.w * input.v0.w);
    r2.xyz = (r2.www * r0.xyz + c0.yyy);
    r0 = (r1 * r2);
    output.oC0 = r0;
    return output;
}
