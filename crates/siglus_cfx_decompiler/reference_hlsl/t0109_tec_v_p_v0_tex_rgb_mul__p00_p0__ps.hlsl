uniform sampler2D s0;

static const float4 c2 = float4(-1.0, 1.0, 0.0, 0.0);

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
    r1.xyz = (input.v0.xyz * -(r0.xyz) + c0.xyz);
    r0 = (r0 * input.v0);
    r1.xyz = (c0.www * r1.xyz + r0.xyz);
    r1.xyz = (r1.xyz + c1.xyz);
    r1.xyz = (r1.xyz + c2.xxx);
    r0.xyz = (r0.www * r1.xyz + c2.yyy);
    output.oC0 = r0;
    return output;
}
