uniform sampler2D s0;
uniform sampler2D s1;

static const float4 c2 = float4(-2.0, 1.0, 0.0, 0.0);

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
    float4 r3 = float4(0.0, 0.0, 0.0, 0.0);

    r0 = tex2D(s0, input.t0.xy);
    r1 = tex2D(s1, input.t1.xy);
    r0 = (r0 * input.v0);
    r2.xyz = (r0.xyz * c2.xxx + c2.yyy);
    r2.xyz = (c1.yyy * r2.xyz + r0.xyz);
    r2.w = dot(c0, r0);
    r3.xyz = lerp(r2.xyz, r2.www, c1.xxx);
    r2.xyz = (r3.xyz + c1.zzz);
    r2.xyz = (r2.xyz + -(c1.www));
    r0.xyz = (r0.www * r2.xyz);
    r0 = (r1 * r0);
    output.oC0 = r0;
    return output;
}
