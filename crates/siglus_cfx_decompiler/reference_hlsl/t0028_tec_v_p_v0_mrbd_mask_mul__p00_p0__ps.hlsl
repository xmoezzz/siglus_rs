uniform sampler2D s0;

static const float4 c2 = float4(-2.0, 1.0, -1.0, 0.0);

struct PS_INPUT {
    float4 v0 : COLOR0;
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

    r0 = tex2D(s0, input.t1.xy);
    r1.xyz = (input.v0.xyz * c2.xxx + c2.yyy);
    r1.xyz = (c1.yyy * r1.xyz + input.v0.xyz);
    r1.w = dot(c0, input.v0);
    r2.xyz = lerp(r1.xyz, r1.www, c1.xxx);
    r1.xyz = (r2.xyz + c1.zzz);
    r1.xyz = (r1.xyz + -(c1.www));
    r1.xyz = (r1.xyz + c2.zzz);
    r1.xyz = (input.v0.www * r1.xyz + c2.yyy);
    r1.w = input.v0.w;
    r0 = (r0 * r1);
    output.oC0 = r0;
    return output;
}
